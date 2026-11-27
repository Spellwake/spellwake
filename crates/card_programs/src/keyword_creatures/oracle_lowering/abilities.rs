use super::*;

fn strip_each_mode_different_player(ops: &[InstructionAst]) -> (Vec<InstructionAst>, bool) {
    let mut out = Vec::with_capacity(ops.len());
    let mut found = false;
    for op in ops {
        if let InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } = op
            && restriction.eq_ignore_ascii_case("EachModeDifferentPlayer")
        {
            found = true;
            continue;
        }
        out.push(op.clone());
    }
    (out, found)
}

fn all_modes_target_single_player(modes: &[ExecutableBody]) -> bool {
    !modes.is_empty()
        && modes.iter().all(|b| {
            matches!(
                b.target,
                BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            )
        })
}

fn modal_choice_requests_distinct(choice: &str) -> bool {
    let low = choice.to_ascii_lowercase();
    low.contains("different player") || low.contains("different target")
}

fn subject_matches_enchanted_land(
    subject: &SubjectRefAst,
    attachment: Option<&AttachmentKind>,
    vocabulary: &CreatureTypeVocabulary,
) -> bool {
    let Some(AttachmentKind::Aura(restriction)) = attachment else {
        return false;
    };
    match subject {
        SubjectRefAst::AttachedHost(desc) => {
            let low = desc.trim().to_ascii_lowercase();
            if low == "enchanted land" {
                return true;
            }
            if let Some(kind_raw) = low.strip_prefix("enchanted ")
                && let EnchantRestriction::Matching(aura_filter) = restriction
                && let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                    kind_raw.trim(),
                    &cached_compiler_vocab(vocabulary),
                )
                && ast_f.zone.is_none()
                && !ast_f.any_spell
                && let Ok(subj_filter) = lower_object_filter(&ast_f, vocabulary)
            {
                return subj_filter == *aura_filter;
            }
            false
        }
        SubjectRefAst::Group(f) => {
            if f.flags != AstFilterFlags::ENCHANTED
                || !filter_only_has(
                    f,
                    FilterAllow {
                        types: true,
                        subtypes: true,
                        flags: true,
                        ..FilterAllow::NONE
                    },
                )
            {
                return false;
            }
            if f.any_types == AstCardTypeMask::LAND && f.subtypes.is_empty() {
                return true;
            }
            if let EnchantRestriction::Matching(aura_filter) = restriction {
                let mut stripped = f.as_ref().clone();
                stripped.flags = AstFilterFlags::EMPTY;
                if let Ok(subj_filter) = lower_object_filter(&stripped, vocabulary) {
                    return subj_filter == *aura_filter;
                }
            }
            false
        }
        _ => false,
    }
}

fn try_lower_triggered_mana_ability(
    trigger: &TriggerSpecAst,
    body: &[InstructionAst],
    entry_tap: Option<&EntryTap>,
    attachment: Option<&AttachmentKind>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Result<ManaOutput, &'static str>> {
    if let [
        TriggerEventAst::TapStateChanged {
            subject,
            tapped: true,
            for_mana: true,
        },
    ] = trigger.events.as_slice()
        && subject_matches_enchanted_land(subject, attachment, vocabulary)
    {
        if trigger.intervening_if.is_some() {
            return Some(Err("oracle:unsupported-triggered-mana"));
        }
        let [
            InstructionAst::AddMana {
                player: PlayerRefAst::ItsController | PlayerRefAst::ThatPlayer | PlayerRefAst::You,
                mana,
            },
        ] = body
        else {
            return Some(Err("oracle:unsupported-triggered-mana"));
        };
        let bonus = match mana {
            ManaSpecAst::Fixed(raw) => {
                let Some(pool) = parse_mana_bundle(raw) else {
                    return Some(Err("oracle:unsupported-triggered-mana"));
                };
                EnchantedLandManaBonus::Fixed(pool)
            }
            ManaSpecAst::ChosenColor => {
                if !matches!(
                    entry_tap,
                    Some(
                        EntryTap::ChooseColor
                            | EntryTap::ChooseColorExcept(_)
                            | EntryTap::AlwaysChooseColorExcept(_)
                    )
                ) {
                    return Some(Err("oracle:unsupported-triggered-mana"));
                }
                EnchantedLandManaBonus::ChosenColor
            }
            ManaSpecAst::AnyColor {
                amount,
                single_color,
            } => {
                let ValueExprAst::Const(n) = amount.as_ref() else {
                    return Some(Err("oracle:unsupported-triggered-mana"));
                };
                if *n == 1 {
                    EnchantedLandManaBonus::AnyColor { count: 1 }
                } else if *n > 1 && !*single_color {
                    let Ok(count) = u8::try_from(*n) else {
                        return Some(Err("oracle:unsupported-triggered-mana"));
                    };
                    EnchantedLandManaBonus::AnyCombination { count }
                } else {
                    // Fail closed on N > 1 mana of any ONE color since triggered_mana_queue
                    // resolves independent single-color choices per unit.
                    return Some(Err("oracle:unsupported-triggered-mana"));
                }
            }
            _ => return Some(Err("oracle:unsupported-triggered-mana")),
        };
        return Some(Ok(ManaOutput::Triggered(
            TriggeredManaRule::EnchantedLandTapped { extra: bonus },
        )));
    }
    let [
        TriggerEventAst::LandTappedForManaColor {
            filter,
            mana_color_filter,
            by_you,
        },
    ] = trigger.events.as_slice()
    else {
        return None;
    };
    if trigger.intervening_if.is_some()
        || !mana_color_filter
            .trim()
            .eq_ignore_ascii_case("chosen_color")
    {
        return Some(Err("oracle:unsupported-triggered-mana"));
    }
    if !matches!(
        entry_tap,
        Some(
            EntryTap::ChooseColor
                | EntryTap::ChooseColorExcept(_)
                | EntryTap::AlwaysChooseColorExcept(_)
        )
    ) {
        return Some(Err("oracle:unsupported-triggered-mana"));
    }
    // `TriggeredManaRule::LandTappedChosenColor` checks any land (not e.g. basic-only lands),
    // so fail closed on any extra filter restriction beyond plain land / controller matching.
    let filter_ok = filter.any_types == AstCardTypeMask::LAND
        && filter_only_has(
            filter,
            FilterAllow {
                types: true,
                controller: true,
                ..FilterAllow::NONE
            },
        )
        && matches!(
            (filter.controller, *by_you),
            (None, _) | (Some(ControllerRelation::You), true)
        );
    if !filter_ok {
        return Some(Err("oracle:unsupported-triggered-mana"));
    }
    let body_player = match body {
        [
            InstructionAst::AddMana {
                player,
                mana: ManaSpecAst::ChosenColor,
            },
        ] => Some(*player),
        [
            InstructionAst::AddManaLandProduced {
                player,
                count: None,
            },
        ] => Some(*player),
        [
            InstructionAst::AddManaLandProduced {
                player,
                count: Some(cnt),
            },
        ] if matches!(cnt.as_ref(), ValueExprAst::Const(1)) => Some(*player),
        _ => None,
    };
    let player_ok = matches!(
        (body_player, *by_you),
        (Some(PlayerRefAst::You), true)
            | (
                Some(PlayerRefAst::ThatPlayer | PlayerRefAst::ItsController),
                false
            )
    );
    if !player_ok {
        return Some(Err("oracle:unsupported-triggered-mana"));
    }
    Some(Ok(ManaOutput::Triggered(
        TriggeredManaRule::LandTappedChosenColor {
            controller_only: *by_you,
        },
    )))
}

fn trigger_event_subject_is_self(ev: &TriggerEventAst) -> bool {
    let mut cur = ev;
    while let TriggerEventAst::QualifiedTrigger { inner, .. } = cur {
        cur = inner.as_ref();
    }
    match cur {
        TriggerEventAst::ZoneChange { subject, .. }
        | TriggerEventAst::PermanentSacrificed { subject, .. }
        | TriggerEventAst::TapStateChanged { subject, .. }
        | TriggerEventAst::CountersChanged { subject, .. }
        | TriggerEventAst::PhaseChanged { subject, .. }
        | TriggerEventAst::Exiled { subject, .. } => {
            matches!(subject.as_ref(), SubjectRefAst::SelfObject)
        }
        TriggerEventAst::Attacks {
            subject: Some(s), ..
        }
        | TriggerEventAst::DamageDealt {
            source: Some(s), ..
        } => matches!(s.as_ref(), SubjectRefAst::SelfObject),
        _ => false,
    }
}

fn is_it_anaphora(subj: &SubjectRefAst) -> bool {
    matches!(subj, SubjectRefAst::Anaphora(raw) if raw.trim().eq_ignore_ascii_case("it"))
}

fn replace_compound_it_with_self(ev: &TriggerEventAst) -> TriggerEventAst {
    match ev {
        TriggerEventAst::QualifiedTrigger {
            inner,
            first_time_each_turn,
            timing,
        } => TriggerEventAst::QualifiedTrigger {
            inner: Box::new(replace_compound_it_with_self(inner)),
            first_time_each_turn: *first_time_each_turn,
            timing: timing.clone(),
        },
        TriggerEventAst::PermanentSacrificed { player, subject } if is_it_anaphora(subject) => {
            TriggerEventAst::PermanentSacrificed {
                player: *player,
                subject: Box::new(SubjectRefAst::SelfObject),
            }
        }
        TriggerEventAst::Attacks {
            subject: Some(subj),
            actor,
            qualifier,
            grouped,
        } if is_it_anaphora(subj) => TriggerEventAst::Attacks {
            subject: Some(Box::new(SubjectRefAst::SelfObject)),
            actor: *actor,
            qualifier: qualifier.clone(),
            grouped: *grouped,
        },
        TriggerEventAst::ZoneChange {
            from,
            to,
            subject,
            grouped,
            entering_controller,
        } if is_it_anaphora(subject) => TriggerEventAst::ZoneChange {
            from: from.clone(),
            to: to.clone(),
            subject: Box::new(SubjectRefAst::SelfObject),
            grouped: *grouped,
            entering_controller: *entering_controller,
        },
        TriggerEventAst::TapStateChanged {
            subject,
            tapped,
            for_mana,
        } if is_it_anaphora(subject) => TriggerEventAst::TapStateChanged {
            subject: Box::new(SubjectRefAst::SelfObject),
            tapped: *tapped,
            for_mana: *for_mana,
        },
        TriggerEventAst::Exiled { subject } if is_it_anaphora(subject) => TriggerEventAst::Exiled {
            subject: Box::new(SubjectRefAst::SelfObject),
        },
        other => other.clone(),
    }
}

fn cost_atom_requires_battlefield(atom: &CostAtomAst) -> bool {
    match atom {
        CostAtomAst::TapSelf
        | CostAtomAst::UntapSelf
        | CostAtomAst::SacrificeSelf
        | CostAtomAst::ExertSelf
        | CostAtomAst::ExileSelf {
            from_graveyard: false,
            from_hand: false,
        } => true,
        CostAtomAst::BounceToHand(subject)
        | CostAtomAst::RemoveCounters { subject, .. }
        | CostAtomAst::PutCounters { subject, .. }
        | CostAtomAst::Unattach { subject, .. } => {
            matches!(subject.as_ref(), SubjectRefAst::SelfObject)
        }
        CostAtomAst::Or(a, b) => {
            cost_atom_requires_battlefield(a) || cost_atom_requires_battlefield(b)
        }
        _ => false,
    }
}

fn cost_atoms_require_battlefield(cost: &[CostAtomAst]) -> bool {
    cost.iter().any(cost_atom_requires_battlefield)
}

fn body_contains_graveyard_self_return_instruction(body: &ExecutableBody) -> bool {
    // Only infer a graveyard-zone self-return when the source moves directly to the battlefield
    // without first leaving the battlefield (blink/flicker). Returning self to hand is a standard
    // battlefield bounce unless the AST explicitly names a graveyard origin.
    let mut source_left_battlefield = false;
    for op in &body.instructions {
        if let Some(spec) =
            crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
            && matches!(spec.source, spellwake_engine::toolbox::ObjectQuery::Source)
        {
            if matches!(
                spec.destination,
                spellwake_engine::toolbox::ZoneDest::Battlefield { .. }
            ) {
                if !source_left_battlefield {
                    return true;
                }
            } else {
                source_left_battlefield = true;
            }
        }
    }
    false
}

pub(super) fn lower_ability(
    ab: &AbilityAst,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    match ab {
        AbilityAst::Activated {
            ability_word,
            zone,
            cost,
            body,
        } => {
            let is_boast = ability_word
                .as_deref()
                .is_some_and(|w| w.eq_ignore_ascii_case("boast"));
            // Try lowering as a ManaAbility first (battlefield, untargeted AddMana)
            let entry_tap = program.permanent().and_then(|p| p.entry_tap.as_ref());
            if zone.is_none()
                && !is_boast
                && let Some(mana_outs) = try_lower_mana_abilities(cost, body, entry_tap, vocabulary)
            {
                for mana_out in mana_outs {
                    program.mana.push(ManaAbility {
                        id: ids.mana(),
                        output: mana_out,
                    });
                }
                return Ok(());
            }
            let (mut filtered_ins, mut restriction, mut cost_reduction) =
                strip_activated_restrictions_ext(body, entry_tap, true, vocabulary)?;
            if is_boast {
                restriction = Some(combine_activation_restrictions(
                    restriction,
                    ActivationRestriction::Boast,
                ));
            }
            let cost_on_battlefield = cost_atoms_require_battlefield(cost);
            let effective_zone = zone.as_deref().or_else(|| {
                (!cost_on_battlefield && body_returns_self_from_graveyard_ext(&filtered_ins))
                    .then_some("graveyard")
            });
            let ((mut mana_cost, mut source_cost, mut hybrids, mut extra_costs), cost_meta) =
                lower_activation_cost_for_ability(cost, effective_zone, vocabulary)?;
            if cost_meta.requires_x_nonzero {
                restriction = Some(combine_activation_restrictions(
                    restriction,
                    ActivationRestriction::XNonZero,
                ));
            }
            if cost_meta.rewrites_counter_prior_to_x {
                rewrite_cost_counter_prior_to_chosen_x(&mut filtered_ins);
            }
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
                let (pre_clean, pre_distinct) = strip_each_mode_different_player(pre);
                let (post_clean, post_distinct) = strip_each_mode_different_player(post);
                let mut distinct_requested =
                    pre_distinct || post_distinct || modal_choice_requests_distinct(choice);
                let mut mode_bodies = Vec::with_capacity(modes.len());
                for m in modes {
                    let (stripped, mode_res, mode_red) =
                        strip_activated_restrictions_ext(&m.body, entry_tap, true, vocabulary)?;
                    if let Some(r) = mode_res {
                        restriction = Some(combine_activation_restrictions(restriction, r));
                    }
                    if let Some(red) = mode_red {
                        cost_reduction = Some(combine_cost_reductions(cost_reduction, red));
                    }
                    let (mode_clean, mode_distinct) = strip_each_mode_different_player(&stripped);
                    distinct_requested |= mode_distinct;
                    mode_bodies.push(mode_clean);
                }
                let allows_modal_split = (min, max) == (1, 1)
                    && !allow_repeat
                    && !distinct_requested
                    && uniqueness == ModalUniqueness::None
                    && !restriction
                        .as_ref()
                        .is_some_and(|r| r.has_once_each_turn() || r.has_exhaust());
                if allows_modal_split {
                    for mb in &mode_bodies {
                        let mut full =
                            Vec::with_capacity(pre_clean.len() + mb.len() + post_clean.len());
                        full.extend_from_slice(&pre_clean);
                        full.extend_from_slice(mb);
                        full.extend_from_slice(&post_clean);
                        let mut exec_body = lower_effect_body(&full, vocabulary, ids, false, true)?;
                        rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
                        let id = ids.activated();
                        if let Some(res) = restriction.clone() {
                            program.activation_restrictions.insert(id, res);
                        }
                        if let Some(red) = cost_reduction.clone() {
                            program.activation_cost_reductions.insert(id, red);
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
                let prefix = if pre_clean.is_empty() {
                    None
                } else {
                    let mut pb = lower_effect_body(&pre_clean, vocabulary, ids, false, true)?;
                    rewrite_body_anaphora_stats(&mut pb, anaphora_ref);
                    if pb.target != BodyTarget::None {
                        return Err("oracle:modal-prefix-target");
                    }
                    Some(Box::new(pb))
                };
                let suffix = if post_clean.is_empty() {
                    None
                } else {
                    let mut sb = lower_effect_body(&post_clean, vocabulary, ids, false, true)?;
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
                let all_single_player = all_modes_target_single_player(&lowered_modes);
                if distinct_requested && !all_single_player {
                    return Err("oracle:unsupported-modal-distinct-targets");
                }
                let distinct_targets = distinct_requested && all_single_player;
                let first = lowered_modes.remove(0);
                let id = ids.activated();
                if let Some(res) = restriction {
                    program.activation_restrictions.insert(id, res);
                }
                if let Some(red) = cost_reduction {
                    program.activation_cost_reductions.insert(id, red);
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
                        distinct_targets,
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
            let (mut exec_body, synthesized_modes) =
                match lower_effect_body(&filtered_ins, vocabulary, ids, false, true) {
                    Ok(body) => (body, None),
                    Err(
                        err @ ("oracle:multiple-targets"
                        | "oracle:conflicting-targets"
                        | "oracle:invalid-fixed-clause"),
                    ) if !extra_costs
                        .iter()
                        .any(ExtraActivationCost::uses_selected_objects)
                        && !source_cost.uses_selected_cost_origin() =>
                    {
                        if let Some((first, modes)) = try_lower_exact_plus_upto_activated_modes(
                            &filtered_ins,
                            vocabulary,
                            ids,
                        )
                        .or_else(|| {
                            try_lower_independent_multi_clause_activated_modes(
                                &filtered_ins,
                                vocabulary,
                                ids,
                            )
                        }) {
                            (first, Some(modes))
                        } else {
                            return Err(err);
                        }
                    }
                    Err(err) => return Err(err),
                };
            rewrite_body_anaphora_stats(&mut exec_body, anaphora_ref);
            let synthesized_modes = synthesized_modes.map(|mut modes| {
                if let Some(prefix) = modes.prefix.as_deref_mut() {
                    rewrite_body_anaphora_stats(prefix, anaphora_ref);
                }
                if let Some(suffix) = modes.suffix.as_deref_mut() {
                    rewrite_body_anaphora_stats(suffix, anaphora_ref);
                }
                for opt in &mut modes.additional {
                    rewrite_body_anaphora_stats(opt, anaphora_ref);
                }
                modes
            });
            if effective_zone.is_none()
                && !cost_on_battlefield
                && synthesized_modes.is_none()
                && !source_cost.is_from_graveyard()
                && !extra_costs
                    .iter()
                    .any(ExtraActivationCost::is_from_graveyard)
                && !extra_costs.iter().any(extra_cost_requires_battlefield)
                && body_contains_graveyard_self_return_instruction(&exec_body)
            {
                let ((gy_mc, gy_sc, gy_hybs, gy_extra), _) =
                    lower_activation_cost_for_ability(cost, Some("graveyard"), vocabulary)?;
                mana_cost = gy_mc;
                source_cost = gy_sc;
                hybrids = gy_hybs;
                extra_costs = gy_extra;
            }
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
            if let Some(red) = cost_reduction {
                program.activation_cost_reductions.insert(id, red);
            }
            if let Some(modes) = synthesized_modes {
                program.activation_modes.insert(id, modes);
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
            let entry_tap = program.permanent().and_then(|p| p.entry_tap.as_ref());
            let attachment = program.permanent().and_then(|p| p.attachment.as_ref());
            if let Some(mana_res) =
                try_lower_triggered_mana_ability(trigger, body, entry_tap, attachment, vocabulary)
            {
                let output = mana_res?;
                program.mana.push(ManaAbility {
                    id: ids.mana(),
                    output,
                });
                return Ok(());
            }
            let resolved_compound_trigger =
                if trigger.events.len() >= 2 && trigger_event_subject_is_self(&trigger.events[0]) {
                    let mut events = Vec::with_capacity(trigger.events.len());
                    events.push(trigger.events[0].clone());
                    for ev in &trigger.events[1..] {
                        events.push(replace_compound_it_with_self(ev));
                    }
                    Some(TriggerSpecAst {
                        events,
                        intervening_if: trigger.intervening_if.clone(),
                    })
                } else {
                    None
                };
            let trigger = resolved_compound_trigger.as_ref().unwrap_or(trigger);
            if trigger.events.len() >= 2 {
                let mut branch_specs = Vec::with_capacity(trigger.events.len());
                let mut all_branches_ok = true;
                for ev in &trigger.events {
                    let sub_trig = TriggerSpecAst {
                        events: vec![ev.clone()],
                        intervening_if: trigger.intervening_if.clone(),
                    };
                    if let Ok(lowered) = lower_trigger_spec(&sub_trig, vocabulary) {
                        branch_specs.push((sub_trig, lowered));
                    } else {
                        all_branches_ok = false;
                        break;
                    }
                }
                let combined_res = lower_trigger_spec(trigger, vocabulary);
                let has_distinct_branch_shape = all_branches_ok
                    && branch_specs.windows(2).any(|w| {
                        let ((_, (s1, g1, _)), (_, (s2, g2, _))) = (&w[0], &w[1]);
                        g1 != g2 || s1.zone != s2.zone || s1.intervening_if != s2.intervening_if
                    });
                if all_branches_ok && (combined_res.is_err() || has_distinct_branch_shape) {
                    let mut built = Vec::with_capacity(branch_specs.len());
                    for (sub_trig, lowered_spec) in branch_specs {
                        let ab = build_single_triggered_ability(
                            &sub_trig,
                            lowered_spec,
                            body,
                            program,
                            vocabulary,
                            ids,
                        )?;
                        if ab.grouping == TriggerGrouping::OneOrMoreOnceEachTurn {
                            // Never split a once-each-turn trigger across multiple abilities.
                            return Err("oracle:unsupported-compound-once-each-turn-trigger");
                        }
                        built.push(ab);
                    }
                    program.triggered.extend(built);
                    return Ok(());
                }
                let lowered_spec = combined_res?;
                let ab = build_single_triggered_ability(
                    trigger,
                    lowered_spec,
                    body,
                    program,
                    vocabulary,
                    ids,
                )?;
                program.triggered.push(ab);
                return Ok(());
            }
            let lowered_spec = lower_trigger_spec(trigger, vocabulary)?;
            let ab = build_single_triggered_ability(
                trigger,
                lowered_spec,
                body,
                program,
                vocabulary,
                ids,
            )?;
            program.triggered.push(ab);
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
            let entry_tap = program.permanent().and_then(|p| p.entry_tap);
            let (loyalty_filtered, loyalty_res, loyalty_red) =
                strip_activated_restrictions_ext(body, entry_tap.as_ref(), true, vocabulary)?;
            if loyalty_red.is_some() {
                return Err("oracle:loyalty-cost-reduction");
            }
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
                let (pre_clean, pre_distinct) = strip_each_mode_different_player(pre);
                let (post_clean, post_distinct) = strip_each_mode_different_player(post);
                let mut distinct_requested =
                    pre_distinct || post_distinct || modal_choice_requests_distinct(choice);
                let mut prepared_modes = Vec::with_capacity(modes.len());
                for m in modes {
                    let (m_filtered, m_res, m_red) = strip_activated_restrictions_ext(
                        &m.body,
                        entry_tap.as_ref(),
                        true,
                        vocabulary,
                    )?;
                    if m_red.is_some() {
                        return Err("oracle:loyalty-cost-reduction");
                    }
                    let (m_clean, m_distinct) = strip_each_mode_different_player(&m_filtered);
                    distinct_requested |= m_distinct;
                    prepared_modes.push((m_clean, m_res));
                }
                if (min, max) == (1, 1)
                    && !allow_repeat
                    && !distinct_requested
                    && uniqueness == ModalUniqueness::None
                {
                    for (m_clean, m_res) in prepared_modes {
                        let combined_res = match (loyalty_res.clone(), m_res) {
                            (a, Some(b)) => Some(combine_activation_restrictions(a, b)),
                            (a, None) => a,
                        };
                        let mut full =
                            Vec::with_capacity(pre_clean.len() + m_clean.len() + post_clean.len());
                        full.extend_from_slice(&pre_clean);
                        full.extend(m_clean);
                        full.extend_from_slice(&post_clean);
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
                let prefix = if pre_clean.is_empty() {
                    None
                } else {
                    let pb = lower_effect_body(&pre_clean, vocabulary, ids, false, true)?;
                    if pb.target != BodyTarget::None {
                        return Err("oracle:modal-prefix-target");
                    }
                    Some(Box::new(pb))
                };
                let suffix = if post_clean.is_empty() {
                    None
                } else {
                    let sb = lower_effect_body(&post_clean, vocabulary, ids, false, true)?;
                    if sb.target != BodyTarget::None {
                        return Err("oracle:modal-suffix-target");
                    }
                    Some(Box::new(sb))
                };
                let mut combined_res = loyalty_res.clone();
                let mut lowered_modes = Vec::with_capacity(prepared_modes.len());
                for (m_clean, m_res) in prepared_modes {
                    if let Some(r) = m_res {
                        combined_res = Some(combine_activation_restrictions(combined_res, r));
                    }
                    lowered_modes.push(lower_effect_body(&m_clean, vocabulary, ids, false, true)?);
                }
                let all_single_player = all_modes_target_single_player(&lowered_modes);
                if distinct_requested && !all_single_player {
                    return Err("oracle:unsupported-modal-distinct-targets");
                }
                let distinct_targets = distinct_requested && all_single_player;
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
                        distinct_targets,
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
            let (exec_body, synthesized_modes) =
                match lower_effect_body(&loyalty_filtered, vocabulary, ids, false, true) {
                    Ok(body) => (body, None),
                    Err(
                        err @ ("oracle:multiple-targets"
                        | "oracle:conflicting-targets"
                        | "oracle:invalid-fixed-clause"),
                    ) => {
                        if let Some((first, modes)) = try_lower_exact_plus_upto_activated_modes(
                            &loyalty_filtered,
                            vocabulary,
                            ids,
                        )
                        .or_else(|| {
                            try_lower_independent_multi_clause_activated_modes(
                                &loyalty_filtered,
                                vocabulary,
                                ids,
                            )
                        }) {
                            (first, Some(modes))
                        } else {
                            return Err(err);
                        }
                    }
                    Err(err) => return Err(err),
                };
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
            if let Some(modes) = synthesized_modes {
                program.activation_modes.insert(id, modes);
            }
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
                let (pre_clean, pre_distinct) = strip_each_mode_different_player(pre);
                let (post_clean, post_distinct) = strip_each_mode_different_player(post);
                let mut distinct_requested =
                    pre_distinct || post_distinct || modal_choice_requests_distinct(choice);
                let (prefix, suffix, embed_wrap) = if pre_clean.is_empty() && post_clean.is_empty()
                {
                    (None, None, false)
                } else if (min, max) == (1, 1) && !allow_repeat {
                    (None, None, true)
                } else {
                    let pb = if pre_clean.is_empty() {
                        None
                    } else {
                        let b = lower_effect_body(&pre_clean, vocabulary, ids, true, true)?;
                        if b.target != BodyTarget::None {
                            return Err("oracle:modal-prefix-target");
                        }
                        Some(Box::new(b))
                    };
                    let sb = if post_clean.is_empty() {
                        None
                    } else {
                        let b = lower_effect_body(&post_clean, vocabulary, ids, true, true)?;
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
                    let (m_clean, m_distinct) = strip_each_mode_different_player(&m_stripped);
                    distinct_requested |= m_distinct;
                    let full = if embed_wrap {
                        let mut f =
                            Vec::with_capacity(pre_clean.len() + m_clean.len() + post_clean.len());
                        f.extend_from_slice(&pre_clean);
                        f.extend(m_clean);
                        f.extend_from_slice(&post_clean);
                        f
                    } else {
                        m_clean
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
                            Instruction::MutatePlayer(Box::new(
                                spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                                    cond,
                                ),
                            )),
                        );
                    }
                    options.push(opt_body);
                }
                let all_single_player = all_modes_target_single_player(&options);
                if distinct_requested && (min > 1 || !all_single_player) {
                    return Err("oracle:unsupported-modal-distinct-targets");
                }
                let distinct_targets = distinct_requested && all_single_player;
                ExecutableBody::with_triggered_modes(TriggeredModes {
                    minimum: min,
                    maximum: max,
                    distinct_targets,
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
                match lower_effect_body(body, vocabulary, ids, true, true) {
                    Ok(mut b) => {
                        if let Some(cond) = guard {
                            prepend_instruction_at(
                                &mut b.instructions,
                                0,
                                Instruction::MutatePlayer(Box::new(
                                    spellwake_engine::toolbox::MutatePlayerSpec::require_condition(
                                        cond,
                                    ),
                                )),
                            );
                        }
                        b
                    }
                    Err(
                        err @ ("oracle:multiple-targets"
                        | "oracle:conflicting-targets"
                        | "oracle:invalid-fixed-clause"),
                    ) => {
                        if let Some(modal_body) = try_lower_exact_plus_upto_triggered_modes(
                            body,
                            vocabulary,
                            ids,
                            &spec_template,
                            true,
                            guard,
                        )
                        .or_else(|| {
                            try_lower_independent_multi_clause_triggered_modes(
                                body,
                                vocabulary,
                                ids,
                                &spec_template,
                                true,
                                guard,
                            )
                        }) {
                            modal_body
                        } else {
                            return Err(err);
                        }
                    }
                    Err(err) => return Err(err),
                }
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

fn build_single_triggered_ability(
    trigger: &TriggerSpecAst,
    spec_tuple: (TriggerSpec, TriggerGrouping, Option<InterveningCondition>),
    body: &[InstructionAst],
    program: &Program,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<TriggeredAbility, &'static str> {
    let (mut spec, mut grouping, guard) = spec_tuple;
    if spec.zone == spellwake_engine::program::TriggerZone::Battlefield
        && (body_returns_self_from_graveyard_ext(body)
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
    let unwrapped_for_self = TriggerSpecAst {
        events: trigger
            .events
            .iter()
            .map(|ev| {
                let mut cur = ev;
                while let TriggerEventAst::QualifiedTrigger { inner, .. } = cur {
                    cur = inner.as_ref();
                }
                cur.clone()
            })
            .collect(),
        intervening_if: None,
    };
    let self_pronoun = trigger_subject_is_self(&unwrapped_for_self);
    let stripped_body = strip_triggered_restrictions(body, &mut spec, &mut grouping, vocabulary);
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
        let (pre_clean, pre_distinct) = strip_each_mode_different_player(pre);
        let (post_clean, post_distinct) = strip_each_mode_different_player(post);
        let mut distinct_requested =
            pre_distinct || post_distinct || modal_choice_requests_distinct(choice);
        let (prefix, suffix, embed_wrap) = if pre_clean.is_empty() && post_clean.is_empty() {
            (None, None, false)
        } else if (min, max) == (1, 1) && !allow_repeat {
            (None, None, true)
        } else {
            let pb = if pre_clean.is_empty() {
                None
            } else {
                let b =
                    lower_trigger_effect_body(&pre_clean, vocabulary, ids, &spec, self_pronoun)?;
                if b.target != BodyTarget::None {
                    return Err("oracle:modal-prefix-target");
                }
                Some(Box::new(b))
            };
            let sb = if post_clean.is_empty() {
                None
            } else {
                let b =
                    lower_trigger_effect_body(&post_clean, vocabulary, ids, &spec, self_pronoun)?;
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
            let (m_clean, m_distinct) = strip_each_mode_different_player(&m_stripped);
            distinct_requested |= m_distinct;
            let full = if embed_wrap {
                let mut f = Vec::with_capacity(pre_clean.len() + m_clean.len() + post_clean.len());
                f.extend_from_slice(&pre_clean);
                f.extend(m_clean);
                f.extend_from_slice(&post_clean);
                f
            } else {
                m_clean
            };
            let mut opt_body =
                lower_trigger_effect_body(&full, vocabulary, ids, &spec, self_pronoun)?;
            if let Some(cond) = guard {
                prepend_instruction_at(
                    &mut opt_body.instructions,
                    0,
                    Instruction::MutatePlayer(Box::new(
                        spellwake_engine::toolbox::MutatePlayerSpec::require_condition(cond),
                    )),
                );
            }
            options.push(opt_body);
        }
        let all_single_player = all_modes_target_single_player(&options);
        if distinct_requested && (min > 1 || !all_single_player) {
            return Err("oracle:unsupported-modal-distinct-targets");
        }
        let distinct_targets = distinct_requested && all_single_player;
        ExecutableBody::with_triggered_modes(TriggeredModes {
            minimum: min,
            maximum: max,
            distinct_targets,
            allow_repeat,
            uniqueness,
            prefix,
            suffix,
            options,
        })
    } else {
        match lower_trigger_effect_body(body, vocabulary, ids, &spec, self_pronoun) {
            Ok(mut b) => {
                if let Some(cond) = guard {
                    prepend_instruction_at(
                        &mut b.instructions,
                        0,
                        Instruction::MutatePlayer(Box::new(
                            spellwake_engine::toolbox::MutatePlayerSpec::require_condition(cond),
                        )),
                    );
                }
                b
            }
            Err(
                err @ ("oracle:multiple-targets"
                | "oracle:conflicting-targets"
                | "oracle:invalid-fixed-clause"),
            ) => {
                if let Some(modal_body) = try_lower_exact_plus_upto_triggered_modes(
                    body,
                    vocabulary,
                    ids,
                    &spec,
                    self_pronoun,
                    guard,
                )
                .or_else(|| {
                    try_lower_independent_multi_clause_triggered_modes(
                        body,
                        vocabulary,
                        ids,
                        &spec,
                        self_pronoun,
                        guard,
                    )
                }) {
                    modal_body
                } else {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    };
    apply_trigger_body_rewrites(&mut exec_body, &spec, grouping, self_pronoun);
    if grouping == TriggerGrouping::OneOrMoreTogether
        && (is_player_attacks_one_or_more_opponents_trigger(&unwrapped_for_self)
            || matches!(
                spec.event,
                TriggerEvent::Attacks {
                    mode: AttackTriggerMode::OpponentsAtController { .. },
                    ..
                }
            ))
    {
        // CR 508.1: in a declare-attackers trigger where an opponent or another player attacks,
        // the attacking player is always the active player while grouping remains OneOrMoreTogether.
        rewrite_body_unique_related_player(&mut exec_body, ResolvingPlayer::ActivePlayer);
    }
    Ok(TriggeredAbility {
        id: ids.triggered(),
        condition: spec,
        grouping,
        body: exec_body,
    })
}

fn is_player_attacks_one_or_more_opponents_trigger(trigger: &TriggerSpecAst) -> bool {
    matches!(
        trigger.events.as_slice(),
        [TriggerEventAst::Attacks {
            subject: Some(subj),
            actor: None,
            qualifier: Some(q),
            ..
        }] if matches!(
            subj.as_ref(),
            SubjectRefAst::Player(
                PlayerRefAst::AnyPlayer
                    | PlayerRefAst::EachPlayer
                    | PlayerRefAst::AnyOpponent
                    | PlayerRefAst::EachOpponent
            )
        ) && q.trim().eq_ignore_ascii_case("one or more of your opponents")
    )
}

fn rewrite_resolving_player_if_unique(player: &mut ResolvingPlayer, replacement: ResolvingPlayer) {
    if *player == ResolvingPlayer::UniqueRelatedPlayer {
        *player = replacement;
    }
}

fn rewrite_affected_players_if_unique(players: &mut AffectedPlayers, replacement: ResolvingPlayer) {
    if let AffectedPlayers::One(p) = players {
        rewrite_resolving_player_if_unique(p, replacement);
    }
}

fn rewrite_instructions_unique_related_player(
    instructions: &mut [Instruction],
    replacement: ResolvingPlayer,
) {
    for op in instructions {
        match op {
            Instruction::CreateTokens { creations } => {
                for creation in creations {
                    if let TokenRecipients::Player(p) = &mut creation.recipients {
                        rewrite_resolving_player_if_unique(p, replacement);
                    }
                }
            }
            Instruction::AdjustLife(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.players,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
                if let spellwake_engine::toolbox::LifeOp::Drain { gain_to, .. } = &mut spec.op {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                        gain_to,
                        &|p| rewrite_resolving_player_if_unique(p, replacement),
                        &|ap| rewrite_affected_players_if_unique(ap, replacement),
                        &|_| {},
                    );
                }
            }
            Instruction::DealDamage(spec) => {
                if let spellwake_engine::toolbox::DamageSourceQuery::Objects(q) = &mut spec.source {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                        q,
                        &|p| rewrite_resolving_player_if_unique(p, replacement),
                        &|ap| rewrite_affected_players_if_unique(ap, replacement),
                        &|_| {},
                    );
                }
                match &mut spec.recipients {
                    spellwake_engine::toolbox::DamageTargetQuery::Objects(q) => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            q,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Players(p) => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            p,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Both { objects, players } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            objects,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            players,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::DamageTargetQuery::Relative(rel) => {
                        if let DamageRecipientsRef::Group(DamageGroup::AffectedPlayers(ap)) =
                            rel.as_mut()
                        {
                            rewrite_affected_players_if_unique(ap, replacement);
                        }
                    }
                    _ => {}
                }
            }
            Instruction::MutatePlayer(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.players,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
                match &mut spec.op {
                    spellwake_engine::toolbox::PlayerMutation::OfferAnyPlayer {
                        cost_body,
                        paid_body,
                        declined_body,
                    } => {
                        rewrite_instructions_unique_related_player(
                            &mut cost_body.instructions,
                            replacement,
                        );
                        rewrite_instructions_unique_related_player(
                            &mut paid_body.instructions,
                            replacement,
                        );
                        if let Some(dec) = declined_body {
                            rewrite_instructions_unique_related_player(
                                &mut dec.instructions,
                                replacement,
                            );
                        }
                    }
                    spellwake_engine::toolbox::PlayerMutation::FlipCoins {
                        win_body,
                        lose_body,
                        ..
                    } => {
                        if let Some(wb) = win_body {
                            rewrite_instructions_unique_related_player(
                                &mut wb.instructions,
                                replacement,
                            );
                        }
                        if let Some(lb) = lose_body {
                            rewrite_instructions_unique_related_player(
                                &mut lb.instructions,
                                replacement,
                            );
                        }
                    }
                    spellwake_engine::toolbox::PlayerMutation::RollDie { bands, .. } => {
                        for band in bands {
                            rewrite_instructions_unique_related_player(
                                &mut band.body.instructions,
                                replacement,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Instruction::ZoneOp(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_zone_op(
                    spec,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
            }
            Instruction::DrawCards(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.players,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
            }
            Instruction::MutateObject(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                    &mut spec.source,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
                match &mut spec.selection {
                    spellwake_engine::toolbox::ZoneSelection::Random { chooser, .. }
                    | spellwake_engine::toolbox::ZoneSelection::Prompt { chooser, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            chooser,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::ZoneSelection::All => {}
                }
                match &mut spec.op {
                    spellwake_engine::toolbox::ObjectMutation::Goad { by } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            by,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::ObjectMutation::Attach { host, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            host,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::ObjectMutation::AttachToPlayer { player } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            player,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::ObjectMutation::GainControl {
                        controller, ..
                    } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            controller,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    _ => {}
                }
            }
            Instruction::ApplyContinuous(spec) => {
                if let Some(sel) = &mut spec.selector {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                        sel,
                        &|p| rewrite_resolving_player_if_unique(p, replacement),
                        &|ap| rewrite_affected_players_if_unique(ap, replacement),
                        &|_| {},
                    );
                }
                match &mut spec.payload {
                    spellwake_engine::toolbox::ContinuousPayload::PlayPermission {
                        player, ..
                    }
                    | spellwake_engine::toolbox::ContinuousPayload::SkipTurns { player, .. }
                    | spellwake_engine::toolbox::ContinuousPayload::SkipStep { player, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            player,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    _ => {}
                }
            }
            Instruction::ForEachPlayerQuery { selector, body, .. } => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    selector,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
                rewrite_instructions_unique_related_player(&mut body.instructions, replacement);
            }
            Instruction::ForEach { query, body, .. } => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                    query,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
                rewrite_instructions_unique_related_player(&mut body.instructions, replacement);
            }
            Instruction::PromptChoice(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                    &mut spec.chooser,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
                match &mut spec.domain {
                    spellwake_engine::toolbox::PromptDomain::Objects { query, .. }
                    | spellwake_engine::toolbox::PromptDomain::Partition { query, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                            query,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    spellwake_engine::toolbox::PromptDomain::Players { selector, .. } => {
                        crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_player_selector(
                            selector,
                            &|p| rewrite_resolving_player_if_unique(p, replacement),
                            &|ap| rewrite_affected_players_if_unique(ap, replacement),
                            &|_| {},
                        );
                    }
                    _ => {}
                }
                for branch in &mut spec.option_branches {
                    rewrite_instructions_unique_related_player(
                        &mut branch.instructions,
                        replacement,
                    );
                }
            }
            Instruction::SetObjectTag { query, .. } => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_object_query(
                    query,
                    &|p| rewrite_resolving_player_if_unique(p, replacement),
                    &|ap| rewrite_affected_players_if_unique(ap, replacement),
                    &|_| {},
                );
            }
            Instruction::OptionalSequence { chooser, .. } => {
                rewrite_resolving_player_if_unique(chooser, replacement);
            }
            Instruction::EmitReflexiveTrigger { body } => {
                rewrite_instructions_unique_related_player(&mut body.instructions, replacement);
            }
            _ => {}
        }
    }
}

fn rewrite_body_unique_related_player(
    exec_body: &mut ExecutableBody,
    replacement: ResolvingPlayer,
) {
    rewrite_instructions_unique_related_player(&mut exec_body.instructions, replacement);
    if let Some(modes) = exec_body.triggered_modes_mut() {
        if let Some(prefix) = modes.prefix.as_deref_mut() {
            rewrite_instructions_unique_related_player(&mut prefix.instructions, replacement);
        }
        if let Some(suffix) = modes.suffix.as_deref_mut() {
            rewrite_instructions_unique_related_player(&mut suffix.instructions, replacement);
        }
        for opt in &mut modes.options {
            rewrite_instructions_unique_related_player(&mut opt.instructions, replacement);
        }
    }
}

pub(super) fn parse_mana_cost_with_snow(
    raw: &str,
) -> Option<(ManaCost, u8, Vec<spellwake_engine::mana::HybridSymbol>, u8)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let symbols = trimmed.strip_prefix('{')?.strip_suffix('}')?;
    let mut snow_count: u8 = 0;
    let mut rest = String::new();
    for sym in symbols.split("}{") {
        if sym.eq_ignore_ascii_case("S") {
            snow_count = snow_count.checked_add(1)?;
        } else {
            rest.push('{');
            rest.push_str(sym);
            rest.push('}');
        }
    }
    if snow_count == 0 {
        let (mc, x, hybs) = parse_mana_cost_full(trimmed)?;
        return Some((mc, x, hybs, 0));
    }
    if rest.is_empty() {
        return Some((ManaCost::free(), 0, Vec::new(), snow_count));
    }
    let (mc, x, hybs) = parse_mana_cost_full(&rest)?;
    Some((mc, x, hybs, snow_count))
}

fn subject_contains_self(subject: &SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::SelfObject => true,
        SubjectRefAst::Anaphora(raw) => matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "~" | "this card"
                | "this creature"
                | "this permanent"
                | "this artifact"
                | "this enchantment"
                | "this land"
                | "this aura"
                | "this equipment"
                | "this planeswalker"
        ),
        SubjectRefAst::InZone { subject: inner, .. } => subject_contains_self(inner),
        SubjectRefAst::Compound(items) => items.iter().any(subject_contains_self),
        SubjectRefAst::TargetOneOf { options, .. } => options.iter().any(subject_contains_self),
        _ => false,
    }
}

fn subject_is_self_in_graveyard(subject: &SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard,
        } => subject_contains_self(inner),
        SubjectRefAst::Compound(items) => items.iter().any(subject_is_self_in_graveyard),
        SubjectRefAst::TargetOneOf { options, .. } => {
            options.iter().any(subject_is_self_in_graveyard)
        }
        _ => false,
    }
}

fn body_returns_self_from_graveyard_ext(body: &[InstructionAst]) -> bool {
    if body_returns_self_from_graveyard(body) {
        return true;
    }
    for ins in body {
        match ins {
            InstructionAst::ReturnZone {
                subject, from_zone, ..
            } if (subject_contains_self(subject)
                && from_zone
                    .as_deref()
                    .is_some_and(|from| from.to_ascii_lowercase().contains("graveyard")))
                || subject_is_self_in_graveyard(subject) =>
            {
                return true;
            }
            InstructionAst::MoveZone { subject, .. }
            | InstructionAst::ShuffleIntoLibrary { subject }
            | InstructionAst::Exile { subject, .. }
                if subject_is_self_in_graveyard(subject) =>
            {
                return true;
            }
            InstructionAst::Optional(inner)
            | InstructionAst::IfPreviousSucceeded(inner)
            | InstructionAst::IfPreviousFailed(inner)
            | InstructionAst::Otherwise(inner)
            | InstructionAst::ReplaceEffect(inner)
            | InstructionAst::ReflexiveTrigger(inner)
            | InstructionAst::Simultaneous(inner)
            | InstructionAst::ScopedDuration { body: inner, .. }
            | InstructionAst::BindX { body: inner, .. }
            | InstructionAst::BindXY { body: inner, .. }
            | InstructionAst::IfCondition {
                then_body: inner, ..
            }
            | InstructionAst::OptionalPlayer { body: inner, .. }
                if body_returns_self_from_graveyard_ext(inner) =>
            {
                return true;
            }
            InstructionAst::Unless {
                body: inner,
                unless_action,
                ..
            } if body_returns_self_from_graveyard_ext(inner)
                || unless_action
                    .as_deref()
                    .is_some_and(body_returns_self_from_graveyard_ext) =>
            {
                return true;
            }
            InstructionAst::Choice(branches)
                if branches
                    .iter()
                    .any(|br| body_returns_self_from_graveyard_ext(br.as_slice())) =>
            {
                return true;
            }
            InstructionAst::Modal { modes, .. }
                if modes
                    .iter()
                    .any(|m| body_returns_self_from_graveyard_ext(&m.body)) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn combine_cost_reductions(prev: Option<ValueExpr>, next: ValueExpr) -> ValueExpr {
    match prev {
        None => next,
        Some(existing) => ValueExpr::Add(Box::new(existing), Box::new(next)),
    }
}

fn scale_world_reduction(delta: &str, base: ValueExpr) -> Result<ValueExpr, &'static str> {
    let mc = parse_fixed_mana_cost(delta).ok_or("oracle:unsupported-cost-reduction")?;
    if !mc.required.is_empty() || !mc.generic.is_positive() || !base.is_world_evaluable() {
        return Err("oracle:unsupported-cost-reduction");
    }
    if mc.generic == 1.into() {
        Ok(base)
    } else {
        Ok(ValueExpr::Scale {
            expr: Box::new(base),
            factor: mc.generic,
        })
    }
}

fn parse_only_spend_mana_type(raw: &str) -> Option<ManaType> {
    let low = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    let rest = low.strip_prefix("spend only ")?;
    let (color_part, tail) = if let Some((c, t)) = rest.split_once(" mana ") {
        (c.trim(), t.trim())
    } else if let Some(c) = rest.strip_suffix(" mana") {
        (c.trim(), "")
    } else if let Some((c, t)) = rest.split_once(" to ") {
        (c.trim(), t.trim())
    } else if let Some((c, t)) = rest.split_once(" on ") {
        (c.trim(), t.trim())
    } else {
        let c = rest
            .strip_suffix(" in this way")
            .or_else(|| rest.strip_suffix(" this way"))?;
        (c.trim(), "this way")
    };
    let valid_tail = matches!(
        tail,
        "" | "on x"
            | "x"
            | "this way"
            | "in this way"
            | "to pay this cost"
            | "pay this cost"
            | "to activate this ability"
            | "activate this ability"
            | "to pay for this ability"
            | "pay for this ability"
    );
    if !valid_tail {
        return None;
    }
    parse_single_mana_type(color_part)
}

fn parse_entry_option_restriction(
    core: &str,
    entry_tap: Option<&EntryTap>,
) -> Option<ActivationRestriction> {
    if let Some(rest) = core.strip_prefix("sourceentryoption:") {
        let idx = rest.trim().parse::<u8>().ok()?;
        return Some(ActivationRestriction::SourceEntryOption(idx));
    }
    let opt_raw = core
        .strip_prefix("if you chose ")
        .or_else(|| core.strip_prefix("while you chose "))?;
    let opt = opt_raw
        .strip_suffix(" as ~ entered")
        .or_else(|| opt_raw.strip_suffix(" as ~ entered the battlefield"))
        .or_else(|| opt_raw.strip_suffix(" as this permanent entered"))
        .or_else(|| opt_raw.strip_suffix(" as it entered"))
        .unwrap_or(opt_raw)
        .trim()
        .trim_matches('"');
    match entry_tap {
        Some(EntryTap::ChooseKhansOrDragons) => match opt {
            "khans" => Some(ActivationRestriction::SourceEntryOption(0)),
            "dragons" => Some(ActivationRestriction::SourceEntryOption(1)),
            _ => None,
        },
        Some(EntryTap::ChooseOddOrEven) => match opt {
            "odd" => Some(ActivationRestriction::SourceEntryOption(0)),
            "even" => Some(ActivationRestriction::SourceEntryOption(1)),
            _ => None,
        },
        Some(EntryTap::ChooseOptions(options)) => {
            let idx = options.iter().position(|o| o.eq_ignore_ascii_case(opt))?;
            let idx_u8 = u8::try_from(idx).ok()?;
            Some(ActivationRestriction::SourceEntryOption(idx_u8))
        }
        _ => None,
    }
}

fn is_instant_only_restriction_local(raw: &str) -> bool {
    let low = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    matches!(
        low.as_str(),
        "instant"
            | "as an instant"
            | "only as an instant"
            | "activate only as an instant"
            | "activate this ability only as an instant"
    )
}

fn is_still_type_restriction_local(raw: &str) -> bool {
    let low = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    matches!(
        low.as_str(),
        "it's still a land"
            | "they're still lands"
            | "it's still a cave land"
            | "it's still a planeswalker"
            | "he's still a planeswalker"
            | "she's still a planeswalker"
            | "it's still an artifact"
            | "they're still artifacts"
            | "it's still an enchantment"
            | "they're still enchantments"
            | "it's still a creature"
            | "they're still creatures"
    )
}

fn step_set_activation_condition(steps: &[Step]) -> ActivationRestriction {
    let mut iter = steps.iter().copied();
    let first = iter.next().unwrap_or(Step::Untap);
    let mut sum = ValueExpr::PlayerStat(PlayerStatExpr::IsInStep {
        step: first,
        player: None,
    });
    for step in iter {
        sum = ValueExpr::Add(
            Box::new(sum),
            Box::new(ValueExpr::PlayerStat(PlayerStatExpr::IsInStep {
                step,
                player: None,
            })),
        );
    }
    ActivationRestriction::Condition(Box::new(ResolutionCondition {
        left: sum,
        comparison: Comparison::GreaterOrEqual,
        right: ValueExpr::literal(1),
    }))
}

fn phase_activation_condition(
    phase: spellwake_engine::program::PhaseCheck,
    negated: bool,
) -> ActivationRestriction {
    ActivationRestriction::Condition(Box::new(ResolutionCondition {
        left: ValueExpr::PlayerStat(PlayerStatExpr::IsInPhase {
            phase,
            player: None,
        }),
        comparison: if negated {
            Comparison::Equal
        } else {
            Comparison::GreaterOrEqual
        },
        right: ValueExpr::literal(if negated { 0 } else { 1 }),
    }))
}

fn parse_single_activation_restriction_ext(
    raw: &str,
    entry_tap: Option<&EntryTap>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ActivationRestriction, &'static str> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    let low = trimmed.to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "xcantbezero" | "x can't be 0" | "x cannot be 0" | "x can't be zero"
    ) {
        return Ok(ActivationRestriction::XNonZero);
    }
    if matches!(
        low.as_str(),
        "you can't activate this ability during combat"
            | "you cannot activate this ability during combat"
            | "not during combat"
    ) {
        return Ok(phase_activation_condition(
            spellwake_engine::program::PhaseCheck::Combat,
            true,
        ));
    }
    if let Some(mt) = parse_only_spend_mana_type(trimmed) {
        return Ok(ActivationRestriction::OnlySpendManaType(mt));
    }
    let core = low
        .strip_prefix("activate this ability ")
        .or_else(|| low.strip_prefix("activate "))
        .or_else(|| low.strip_prefix("use this ability "))
        .unwrap_or(&low)
        .trim();
    let core = core.strip_prefix("only ").unwrap_or(core).trim();
    match core {
        "no more than twice each turn" | "twice each turn" => {
            return Ok(ActivationRestriction::MaxPerTurn(2));
        }
        "no more than three times each turn" | "three times each turn" => {
            return Ok(ActivationRestriction::MaxPerTurn(3));
        }
        "no more than four times each turn" | "four times each turn" => {
            return Ok(ActivationRestriction::MaxPerTurn(4));
        }
        "before combat" | "before the combat phase" => {
            return Ok(step_set_activation_condition(&[
                Step::Untap,
                Step::Upkeep,
                Step::Draw,
                Step::PrecombatMain,
            ]));
        }
        "before attackers are declared" | "before the declare attackers step" => {
            return Ok(step_set_activation_condition(&[
                Step::Untap,
                Step::Upkeep,
                Step::Draw,
                Step::PrecombatMain,
                Step::BeginningCombat,
            ]));
        }
        "before blockers are declared" | "before the declare blockers step" => {
            return Ok(step_set_activation_condition(&[
                Step::Untap,
                Step::Upkeep,
                Step::Draw,
                Step::PrecombatMain,
                Step::BeginningCombat,
                Step::DeclareAttackers,
            ]));
        }
        "before the combat damage step" => {
            return Ok(step_set_activation_condition(&[
                Step::Untap,
                Step::Upkeep,
                Step::Draw,
                Step::PrecombatMain,
                Step::BeginningCombat,
                Step::DeclareAttackers,
                Step::DeclareBlockers,
            ]));
        }
        "before the end of combat step" => {
            return Ok(step_set_activation_condition(&[
                Step::Untap,
                Step::Upkeep,
                Step::Draw,
                Step::PrecombatMain,
                Step::BeginningCombat,
                Step::DeclareAttackers,
                Step::DeclareBlockers,
                Step::CombatDamage,
            ]));
        }
        "before the end step" => {
            return Ok(phase_activation_condition(
                spellwake_engine::program::PhaseCheck::Ending,
                true,
            ));
        }
        "during combat after blockers are declared"
        | "during combat, after blockers are declared" => {
            return Ok(step_set_activation_condition(&[
                Step::DeclareBlockers,
                Step::CombatDamage,
                Step::EndCombat,
            ]));
        }
        "during their draw step" => {
            return Ok(ActivationRestriction::DuringOwnStep(Step::Draw));
        }
        "during their turn before the end step" | "during your turn before the end step" => {
            return Ok(combine_activation_restrictions(
                Some(ActivationRestriction::DuringYourTurn),
                phase_activation_condition(spellwake_engine::program::PhaseCheck::Ending, true),
            ));
        }
        _ => {}
    }
    if let Some(opt_res) = parse_entry_option_restriction(core, entry_tap) {
        return Ok(opt_res);
    }
    if let Ok(parsed) = parse_activation_restriction(trimmed) {
        return Ok(parsed);
    }
    if let Some(cond_raw) = core
        .strip_prefix("if ")
        .or_else(|| core.strip_prefix("while "))
    {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        if let Ok(cond_ast) =
            spellwake_oracle_compiler::filter::parse_condition(cond_raw.trim(), &compiler_vocab)
        {
            return lower_conditional_activation_restriction(&cond_ast, vocabulary);
        }
    }
    Err("oracle:unsupported-act-restriction")
}

fn parse_activation_restriction_ext(
    raw: &str,
    entry_tap: Option<&EntryTap>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ActivationRestriction, &'static str> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    if let Some(rest) = trimmed
        .strip_prefix("AnyPlayer:")
        .or_else(|| trimmed.strip_prefix("anyplayer:"))
    {
        let rest_trimmed = rest.trim();
        if rest_trimmed.is_empty() {
            return Ok(ActivationRestriction::AnyPlayer);
        }
        let rest_low = rest_trimmed.to_ascii_lowercase();
        let inner = if matches!(
            rest_low.as_str(),
            "during their turn" | "only during their turn"
        ) {
            ActivationRestriction::DuringYourTurn
        } else {
            parse_activation_restriction_ext(rest_trimmed, entry_tap, vocabulary)?
        };
        return Ok(combine_activation_restrictions(
            Some(ActivationRestriction::AnyPlayer),
            inner,
        ));
    }
    if let Ok(single) = parse_single_activation_restriction_ext(trimmed, entry_tap, vocabulary) {
        return Ok(single);
    }
    let low = trimmed.to_ascii_lowercase();
    let (has_any_player, body) = if let Some(rest) = low
        .strip_prefix("any player may activate this ability, but only ")
        .or_else(|| low.strip_prefix("any player may activate this ability but only "))
    {
        (true, rest.trim())
    } else if let Some(rest) = low
        .strip_prefix("activate this ability only ")
        .or_else(|| low.strip_prefix("activate only "))
        .or_else(|| low.strip_prefix("use this ability only "))
    {
        (false, rest.trim())
    } else {
        (false, low.as_str())
    };
    let normalized = body
        .replace(", and only ", "+")
        .replace(" and only ", "+")
        .replace(", only ", "+")
        .replace(", before ", "+before ");
    let parts: Vec<&str> = normalized
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 || has_any_player {
        let mut acc: Option<ActivationRestriction> =
            has_any_player.then_some(ActivationRestriction::AnyPlayer);
        for part in parts {
            if is_instant_only_restriction_local(part) {
                continue;
            }
            let next_r = parse_single_activation_restriction_ext(part, entry_tap, vocabulary)?;
            acc = Some(combine_activation_restrictions(acc, next_r));
        }
        return acc.ok_or("oracle:unsupported-act-restriction");
    }
    Err("oracle:unsupported-act-restriction")
}

fn try_lower_inline_cost_reduction_str(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ValueExpr> {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    let low = trimmed.to_ascii_lowercase();
    let after = low.strip_prefix("this ability costs ")?;
    let (delta_tok, tail) = after.split_once(' ')?;
    let delta_up = delta_tok.trim().to_ascii_uppercase();
    let mc = parse_fixed_mana_cost(&delta_up)?;
    if !mc.required.is_empty() || !mc.generic.is_positive() {
        return None;
    }
    let compiler_vocab = cached_compiler_vocab(vocabulary);
    if let Some(cond_raw) = tail.trim().strip_prefix("less to activate if ") {
        let cond_ast =
            spellwake_oracle_compiler::filter::parse_condition(cond_raw.trim(), &compiler_vocab)
                .ok()?;
        let mut tmp_builder = BodyBuilder::new();
        let rc = lower_resolution_condition(&cond_ast, &mut tmp_builder, vocabulary, false, None)
            .ok()?;
        if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
            return None;
        }
        return Some(ValueExpr::Conditional {
            left: Box::new(rc.left),
            comparison: rc.comparison,
            right: Box::new(rc.right),
            then_value: Box::new(ValueExpr::Literal(mc.generic)),
            else_value: Box::new(ValueExpr::literal(0)),
        });
    }
    if let Some(fe_raw) = tail.trim().strip_prefix("less to activate for each ") {
        let flt =
            spellwake_oracle_compiler::filter::parse_object_filter(fe_raw.trim(), &compiler_vocab)
                .ok()?;
        let base = lower_value_expr(&ValueExprAst::Count(Box::new(flt)), vocabulary).ok()?;
        return scale_world_reduction(&delta_up, base).ok();
    }
    None
}

type StrippedActivatedRestrictions = (
    Vec<InstructionAst>,
    Option<ActivationRestriction>,
    Option<ValueExpr>,
);

fn strip_activated_restrictions_ext(
    ins: &[InstructionAst],
    entry_tap: Option<&EntryTap>,
    allow_spend_mana_type: bool,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<StrippedActivatedRestrictions, &'static str> {
    strip_activated_restrictions_inner(ins, entry_tap, allow_spend_mana_type, vocabulary, false)
}

fn strip_activated_restrictions_inner(
    ins: &[InstructionAst],
    entry_tap: Option<&EntryTap>,
    allow_spend_mana_type: bool,
    vocabulary: &CreatureTypeVocabulary,
    mut after_exile_or_cast_perm: bool,
) -> Result<StrippedActivatedRestrictions, &'static str> {
    let mut restriction = None;
    let mut cost_reduction = None;
    let mut filtered = Vec::with_capacity(ins.len());
    for item in ins {
        match item {
            InstructionAst::AbilityCostReduction { delta, per } => {
                let base = lower_value_expr(&ValueExprAst::Count(per.clone()), vocabulary)
                    .map_err(|_| "oracle:unsupported-cost-reduction")?;
                let expr = scale_world_reduction(delta, base)?;
                cost_reduction = Some(combine_cost_reductions(cost_reduction, expr));
            }
            InstructionAst::DynamicCostReduction { delta, per } => {
                let base = lower_value_expr(per, vocabulary)
                    .map_err(|_| "oracle:unsupported-cost-reduction")?;
                let expr = scale_world_reduction(delta, base)?;
                cost_reduction = Some(combine_cost_reductions(cost_reduction, expr));
            }
            InstructionAst::TemporaryStatic {
                static_ability,
                duration:
                    DurationAst::Instant | DurationAst::Permanent | DurationAst::UntilEndOfTurn,
            } if matches!(
                static_ability.as_ref(),
                StaticAbilityAst::CostModifier {
                    subject,
                    less: true,
                    next_only: false,
                    ..
                } if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(s) if s.trim().eq_ignore_ascii_case("this ability")
                )
            ) =>
            {
                let StaticAbilityAst::CostModifier {
                    delta,
                    per,
                    scale_expr,
                    condition,
                    ..
                } = static_ability.as_ref()
                else {
                    return Err("oracle:unsupported-cost-reduction");
                };
                let amount = if delta.trim().eq_ignore_ascii_case("{x}")
                    && per.is_none()
                    && let Some(se) = scale_expr.as_deref()
                {
                    lower_value_expr(se, vocabulary)
                        .map_err(|_| "oracle:unsupported-cost-reduction")?
                } else {
                    let mc =
                        parse_fixed_mana_cost(delta).ok_or("oracle:unsupported-cost-reduction")?;
                    if !mc.required.is_empty() || !mc.generic.is_positive() {
                        return Err("oracle:unsupported-cost-reduction");
                    }
                    match (per.as_deref(), scale_expr.as_deref()) {
                        (None, None) => ValueExpr::Literal(mc.generic),
                        (Some(pf), None) => {
                            let base = lower_value_expr(
                                &ValueExprAst::Count(Box::new(pf.clone())),
                                vocabulary,
                            )
                            .map_err(|_| "oracle:unsupported-cost-reduction")?;
                            scale_world_reduction(delta, base)?
                        }
                        (None, Some(se)) => {
                            let base = lower_value_expr(se, vocabulary)
                                .map_err(|_| "oracle:unsupported-cost-reduction")?;
                            scale_world_reduction(delta, base)?
                        }
                        (Some(_), Some(_)) => return Err("oracle:unsupported-cost-reduction"),
                    }
                };
                let final_expr = if let Some(cond) = condition.as_deref() {
                    let mut tmp_builder = BodyBuilder::new();
                    let rc =
                        lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)
                            .map_err(|_| "oracle:unsupported-cost-reduction")?;
                    if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
                        return Err("oracle:unsupported-cost-reduction");
                    }
                    ValueExpr::Conditional {
                        left: Box::new(rc.left),
                        comparison: rc.comparison,
                        right: Box::new(rc.right),
                        then_value: Box::new(amount),
                        else_value: Box::new(ValueExpr::literal(0)),
                    }
                } else {
                    amount
                };
                cost_reduction = Some(combine_cost_reductions(cost_reduction, final_expr));
            }
            InstructionAst::ManaSpendRestriction(r) if allow_spend_mana_type => {
                if let Some(mt) = parse_only_spend_mana_type(r) {
                    restriction = Some(combine_activation_restrictions(
                        restriction,
                        ActivationRestriction::OnlySpendManaType(mt),
                    ));
                } else {
                    filtered.push(item.clone());
                }
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if after_exile_or_cast_perm && is_sorcery_timing_restriction(r) => {
                filtered.push(item.clone());
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if is_instant_only_restriction_local(r) => {}
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if is_still_type_restriction_local(r)
                || r.eq_ignore_ascii_case("EachModeDifferentPlayer")
                || matches!(
                    r.as_str(),
                    "ExcessDamageToController"
                        | "DamageCantBePreventedThisTurn"
                        | "DamageCantBePrevented"
                        | "NoMaxHandSizeRestOfGame"
                ) =>
            {
                filtered.push(item.clone());
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } => {
                if let Some(red) = try_lower_inline_cost_reduction_str(r, vocabulary) {
                    cost_reduction = Some(combine_cost_reductions(cost_reduction, red));
                } else {
                    let next_r = parse_activation_restriction_ext(r, entry_tap, vocabulary)?;
                    restriction = Some(combine_activation_restrictions(restriction, next_r));
                }
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: Some(cond),
            } => {
                let base = r.strip_prefix("Conditional").unwrap_or(r.as_str()).trim();
                if !base.is_empty()
                    && !base.eq_ignore_ascii_case("condition")
                    && !base.eq_ignore_ascii_case("only_if")
                    && !base.eq_ignore_ascii_case("if")
                    && !is_instant_only_restriction_local(base)
                {
                    let base_r = parse_activation_restriction_ext(base, entry_tap, vocabulary)?;
                    restriction = Some(combine_activation_restrictions(restriction, base_r));
                }
                let next_r = lower_conditional_activation_restriction(cond, vocabulary)?;
                restriction = Some(combine_activation_restrictions(restriction, next_r));
            }
            InstructionAst::BindX { x_expr, body } if !body.is_empty() => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    body,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if let Some(ired) = inner_red {
                    cost_reduction = Some(combine_cost_reductions(cost_reduction, ired));
                }
                filtered.push(InstructionAst::BindX {
                    x_expr: x_expr.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::BindXY {
                x_expr,
                y_expr,
                body,
            } if !body.is_empty() => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    body,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if let Some(ired) = inner_red {
                    cost_reduction = Some(combine_cost_reductions(cost_reduction, ired));
                }
                filtered.push(InstructionAst::BindXY {
                    x_expr: x_expr.clone(),
                    y_expr: y_expr.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ScopedDuration { duration, body } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    body,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !body.is_empty()) {
                    filtered.push(InstructionAst::ScopedDuration {
                        duration: duration.clone(),
                        body: inner_filtered,
                    });
                }
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => {
                let (t, tr, tred) = strip_activated_restrictions_inner(
                    then_body,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if t.is_empty()
                    && tr.is_none()
                    && let Some(inner_red) = tred
                {
                    let mut tmp_builder = BodyBuilder::new();
                    let rc = lower_resolution_condition(
                        condition,
                        &mut tmp_builder,
                        vocabulary,
                        false,
                        None,
                    )
                    .map_err(|_| "oracle:unsupported-cost-reduction")?;
                    if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
                        return Err("oracle:unsupported-cost-reduction");
                    }
                    let cond_red = ValueExpr::Conditional {
                        left: Box::new(rc.left),
                        comparison: rc.comparison,
                        right: Box::new(rc.right),
                        then_value: Box::new(inner_red),
                        else_value: Box::new(ValueExpr::literal(0)),
                    };
                    cost_reduction = Some(combine_cost_reductions(cost_reduction, cond_red));
                    continue;
                }
                if tred.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = tr {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::IfCondition {
                    condition: condition.clone(),
                    then_body: t,
                });
            }
            InstructionAst::Optional(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Optional(inner_filtered));
                }
            }
            InstructionAst::IfPreviousSucceeded(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::IfPreviousSucceeded(inner_filtered));
                }
            }
            InstructionAst::IfPreviousFailed(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::IfPreviousFailed(inner_filtered));
                }
            }
            InstructionAst::OptionalPlayer {
                player,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::OptionalPlayer {
                        player: *player,
                        body: inner_filtered,
                    });
                }
            }
            InstructionAst::Otherwise(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Otherwise(inner_filtered));
                }
            }
            InstructionAst::ReplaceEffect(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ReplaceEffect(inner_filtered));
                }
            }
            InstructionAst::ReplaceEffectWith(rew) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    &rew.body,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !rew.body.is_empty()) {
                    let mut next = (**rew).clone();
                    next.body = inner_filtered;
                    filtered.push(InstructionAst::ReplaceEffectWith(Box::new(next)));
                }
            }
            InstructionAst::OneShotReplacement {
                subject,
                event,
                replace_with,
                instead_body,
            } => {
                let new_body = match instead_body {
                    Some(inner) => {
                        let (inner_filtered, inner_r, inner_red) =
                            strip_activated_restrictions_inner(
                                inner,
                                entry_tap,
                                allow_spend_mana_type,
                                vocabulary,
                                after_exile_or_cast_perm,
                            )?;
                        if inner_red.is_some() {
                            return Err("oracle:unsupported-cost-reduction");
                        }
                        if let Some(ir) = inner_r {
                            restriction = Some(combine_activation_restrictions(restriction, ir));
                        }
                        Some(inner_filtered)
                    }
                    None => None,
                };
                filtered.push(InstructionAst::OneShotReplacement {
                    subject: subject.clone(),
                    event: event.clone(),
                    replace_with: replace_with.clone(),
                    instead_body: new_body,
                });
            }
            InstructionAst::SearchLibrary { follow_up, .. } if !follow_up.is_empty() => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    follow_up,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                let mut cloned = item.clone();
                if let InstructionAst::SearchLibrary { follow_up: dst, .. } = &mut cloned {
                    *dst = inner_filtered;
                }
                filtered.push(cloned);
            }
            InstructionAst::ReflexiveTrigger(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ReflexiveTrigger(inner_filtered));
                }
            }
            InstructionAst::ReflexiveEventTrigger {
                trigger,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ReflexiveEventTrigger {
                    trigger: trigger.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::Simultaneous(inner) => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Simultaneous(inner_filtered));
                }
            }
            InstructionAst::Choice(branches) => {
                let mut new_branches = Vec::with_capacity(branches.len());
                for br in branches {
                    let (bf, br_r, br_red) = strip_activated_restrictions_inner(
                        br,
                        entry_tap,
                        allow_spend_mana_type,
                        vocabulary,
                        after_exile_or_cast_perm,
                    )?;
                    if br_red.is_some() {
                        return Err("oracle:unsupported-cost-reduction");
                    }
                    if let Some(ir) = br_r {
                        restriction = Some(combine_activation_restrictions(restriction, ir));
                    }
                    new_branches.push(bf);
                }
                filtered.push(InstructionAst::Choice(new_branches));
            }
            InstructionAst::DelayedTrigger {
                step,
                scope,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::DelayedTrigger {
                    step: *step,
                    scope: *scope,
                    body: inner_filtered,
                });
            }
            InstructionAst::DelayedEventTrigger {
                trigger,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::DelayedEventTrigger {
                    trigger: trigger.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::TemporaryTrigger {
                trigger,
                body: inner,
                duration,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::TemporaryTrigger {
                    trigger: trigger.clone(),
                    body: inner_filtered,
                    duration: duration.clone(),
                });
            }
            InstructionAst::WhenYouNext {
                event,
                body: inner,
                duration,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::WhenYouNext {
                    event: event.clone(),
                    body: inner_filtered,
                    duration: duration.clone(),
                });
            }
            InstructionAst::ForAsLongAs {
                condition,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForAsLongAs {
                    condition: condition.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::SpreeMode { cost, body: inner } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::SpreeMode {
                    cost: cost.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachPlayer {
                player,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachPlayer {
                    player: *player,
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachObject {
                filter,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachObject {
                    filter: filter.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachCount { count, body: inner } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachCount {
                    count: count.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachSubjectPlayer {
                players,
                body: inner,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachSubjectPlayer {
                    players: players.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::Unless {
                body: inner,
                unless_player,
                unless_cost,
                unless_action,
                unless_condition,
            } => {
                let (inner_filtered, inner_r, inner_red) = strip_activated_restrictions_inner(
                    inner,
                    entry_tap,
                    allow_spend_mana_type,
                    vocabulary,
                    after_exile_or_cast_perm,
                )?;
                if inner_red.is_some() {
                    return Err("oracle:unsupported-cost-reduction");
                }
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                let new_action = match unless_action {
                    Some(act) => {
                        let (af, ar, ared) = strip_activated_restrictions_inner(
                            act,
                            entry_tap,
                            allow_spend_mana_type,
                            vocabulary,
                            after_exile_or_cast_perm,
                        )?;
                        if ared.is_some() {
                            return Err("oracle:unsupported-cost-reduction");
                        }
                        if let Some(ir) = ar {
                            restriction = Some(combine_activation_restrictions(restriction, ir));
                        }
                        Some(af)
                    }
                    None => None,
                };
                filtered.push(InstructionAst::Unless {
                    body: inner_filtered,
                    unless_player: *unless_player,
                    unless_cost: unless_cost.clone(),
                    unless_action: new_action,
                    unless_condition: unless_condition.clone(),
                });
            }
            InstructionAst::Modal { choice, modes } => {
                let mut new_modes = Vec::with_capacity(modes.len());
                for m in modes {
                    let (mb, mr, mred) = strip_activated_restrictions_inner(
                        &m.body,
                        entry_tap,
                        allow_spend_mana_type,
                        vocabulary,
                        after_exile_or_cast_perm,
                    )?;
                    if let Some(ir) = mr {
                        restriction = Some(combine_activation_restrictions(restriction, ir));
                    }
                    if let Some(ired) = mred {
                        cost_reduction = Some(combine_cost_reductions(cost_reduction, ired));
                    }
                    new_modes.push(spellwake_oracle_compiler::ast::ModalModeAst {
                        body: mb,
                        label: m.label.clone(),
                        granted: m.granted.clone(),
                    });
                }
                filtered.push(InstructionAst::Modal {
                    choice: choice.clone(),
                    modes: new_modes,
                });
            }
            _ => {
                filtered.push(item.clone());
            }
        }
        if is_exile_or_cast_perm_instr(item) {
            after_exile_or_cast_perm = true;
        }
    }
    Ok((filtered, restriction, cost_reduction))
}

pub(super) fn try_lower_mana_ability(
    cost: &[CostAtomAst],
    body: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ManaOutput> {
    let mut outs = try_lower_mana_abilities(cost, body, None, vocabulary)?;
    if outs.len() == 1 { outs.pop() } else { None }
}

fn try_lower_mana_abilities(
    cost: &[CostAtomAst],
    body: &[InstructionAst],
    entry_tap: Option<&EntryTap>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Vec<ManaOutput>> {
    fn restriction_has_any_player(r: &ActivationRestriction) -> bool {
        match r {
            ActivationRestriction::AnyPlayer => true,
            ActivationRestriction::All(items) => items.iter().any(restriction_has_any_player),
            _ => false,
        }
    }
    let (filtered_body, act_restriction, cost_reduction) =
        strip_activated_restrictions_ext(body, entry_tap, false, vocabulary).ok()?;
    if cost_reduction.is_some()
        || act_restriction
            .as_ref()
            .is_some_and(restriction_has_any_player)
    {
        return None;
    }
    // Extract AddMana (or qualified fixed-or-chosen-color mana choice) + optional SpendRestriction / SpendEffect + optional Life/Damage side effect
    let mut mana_spec: Option<&ManaSpecAst> = None;
    let mut entry_chosen_fixed: Option<ManaType> = None;
    let mut restriction: Option<spellwake_engine::mana::ManaOutputRestriction> = None;
    let mut spend_effect: Option<spellwake_engine::mana::ManaSpendEffect> = None;
    let mut side_effect: Option<ManaSideEffect> = None;
    for ins in &filtered_body {
        match ins {
            InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana,
            } => {
                if mana_spec.is_some() || entry_chosen_fixed.is_some() {
                    return None;
                }
                mana_spec = Some(mana);
            }
            InstructionAst::AddQualifiedMana(qm)
                if qm.player == PlayerRefAst::You
                    && qm.kind.eq_ignore_ascii_case("mana_choice")
                    && qm.amount.is_none()
                    && qm.source.is_none() =>
            {
                if mana_spec.is_some() || entry_chosen_fixed.is_some() {
                    return None;
                }
                let fixed_raw = match qm.choices.as_slice() {
                    [ManaSpecAst::Fixed(raw), ManaSpecAst::ChosenColor]
                    | [ManaSpecAst::ChosenColor, ManaSpecAst::Fixed(raw)] => raw,
                    _ => return None,
                };
                let fixed = parse_single_mana_type(fixed_raw)?;
                if fixed == ManaType::Colourless {
                    return None;
                }
                let entry_color_ok = match entry_tap {
                    Some(EntryTap::ChooseColor) => true,
                    Some(EntryTap::AlwaysChooseColorExcept(c) | EntryTap::ChooseColorExcept(c)) => {
                        *c == fixed
                    }
                    _ => false,
                };
                if !entry_color_ok {
                    return None;
                }
                entry_chosen_fixed = Some(fixed);
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
                    } else if let Some(dynamic_r) =
                        interactive::parse_dynamic_mana_output_restriction(r, vocabulary)
                    {
                        restriction = Some(dynamic_r);
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
    let base_outputs: Vec<ManaOutput> = if let Some(fixed) = entry_chosen_fixed {
        vec![ManaOutput::SpecialColor(
            SpecialColorYield::EntryChosenColor { fixed },
        )]
    } else {
        let spec = mana_spec?;
        match spec {
            ManaSpecAst::Fixed(raw) => vec![ManaOutput::Fixed(parse_mana_bundle(raw)?)],
            ManaSpecAst::Choice(opts) => {
                if opts.is_empty() {
                    return None;
                }
                let mut single_colors = BTreeSet::new();
                let mut extra_bundles: Vec<BTreeMap<ManaType, RulesInteger>> = Vec::new();
                let mut extra_all_color_amounts: BTreeSet<RulesInteger> = BTreeSet::new();
                let mut has_chosen_color = false;
                for o in opts {
                    let o_low = o.trim().to_ascii_lowercase();
                    if matches!(
                        o_low.as_str(),
                        "one mana of any color" | "one mana of any one color"
                    ) {
                        single_colors.extend(ManaOutput::all_colors());
                    } else if o_low == "two mana of any one color" {
                        extra_all_color_amounts.insert(2.into());
                    } else if o_low == "three mana of any one color" {
                        extra_all_color_amounts.insert(3.into());
                    } else if matches!(
                        o_low.as_str(),
                        "chosen_color" | "one mana of the chosen color" | "one mana of that color"
                    ) {
                        has_chosen_color = true;
                    } else if let Some(mt) = parse_single_mana_type(o) {
                        if mt == ManaType::Colourless {
                            let bundle = BTreeMap::from([(ManaType::Colourless, 1.into())]);
                            if !extra_bundles.contains(&bundle) {
                                extra_bundles.push(bundle);
                            }
                        } else {
                            single_colors.insert(mt);
                        }
                    } else {
                        let bundle = parse_mana_bundle(o)?;
                        if !extra_bundles.contains(&bundle) {
                            extra_bundles.push(bundle);
                        }
                    }
                }
                if has_chosen_color {
                    if !extra_bundles.is_empty()
                        || !extra_all_color_amounts.is_empty()
                        || single_colors.len() != 1
                    {
                        return None;
                    }
                    let fixed = *single_colors.iter().next()?;
                    if !matches!(
                        entry_tap,
                        Some(EntryTap::AlwaysChooseColorExcept(c) | EntryTap::ChooseColorExcept(c))
                            if *c == fixed
                    ) {
                        return None;
                    }
                    vec![ManaOutput::SpecialColor(
                        SpecialColorYield::EntryChosenColor { fixed },
                    )]
                } else if extra_bundles.is_empty() && extra_all_color_amounts.is_empty() {
                    if single_colors.is_empty() {
                        return None;
                    }
                    vec![ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors: single_colors,
                    }]
                } else {
                    let mut outs = Vec::new();
                    let mut grouped_colored: BTreeMap<RulesInteger, BTreeSet<ManaType>> =
                        BTreeMap::new();
                    let mut fixed_bundles: Vec<BTreeMap<ManaType, RulesInteger>> = Vec::new();
                    for bundle in extra_bundles {
                        if bundle.len() == 1
                            && let Some((&mt, qty)) = bundle.iter().next()
                            && mt != ManaType::Colourless
                        {
                            grouped_colored.entry(qty.clone()).or_default().insert(mt);
                        } else {
                            fixed_bundles.push(bundle);
                        }
                    }
                    if !single_colors.is_empty() {
                        grouped_colored
                            .entry(1.into())
                            .or_default()
                            .extend(single_colors);
                    }
                    for amt in extra_all_color_amounts {
                        grouped_colored
                            .entry(amt)
                            .or_default()
                            .extend(ManaOutput::all_colors());
                    }
                    for bundle in fixed_bundles {
                        outs.push(ManaOutput::Fixed(bundle));
                    }
                    for (amount, colors) in grouped_colored {
                        if colors.len() == 1 {
                            let c = *colors.iter().next()?;
                            outs.push(ManaOutput::Fixed(BTreeMap::from([(c, amount)])));
                        } else if !colors.is_empty() {
                            outs.push(ManaOutput::ChooseColor { amount, colors });
                        }
                    }
                    if outs.is_empty() {
                        return None;
                    }
                    outs
                }
            }
            ManaSpecAst::AnyColor {
                amount,
                single_color,
            } if *single_color || matches!(amount.as_ref(), ValueExprAst::Const(1)) => {
                let ValueExprAst::Const(n) = amount.as_ref() else {
                    return None;
                };
                if *n <= 0 {
                    return None;
                }
                vec![ManaOutput::ChooseColor {
                    amount: i64::from(*n).into(),
                    colors: ManaOutput::all_colors(),
                }]
            }
            ManaSpecAst::DifferentColors { amount }
                if matches!(amount.as_ref(), ValueExprAst::Const(2)) =>
            {
                vec![ManaOutput::TwoColors { distinct: true }]
            }
            ManaSpecAst::CombinationOf { amount, symbols }
                if matches!(amount.as_ref(), ValueExprAst::Const(2))
                    && symbols.len() == 5
                    && symbols.iter().all(|s| {
                        parse_single_mana_type(s).is_some_and(|m| m != ManaType::Colourless)
                    }) =>
            {
                vec![ManaOutput::TwoColors { distinct: false }]
            }
            ManaSpecAst::ScaledSymbol { symbol, amount } => {
                let mt = parse_single_mana_type(symbol)?;
                let ValueExprAst::Count(filter) = amount.as_ref() else {
                    return None;
                };
                let permanents = lower_filter_to_permanent_selector(filter, vocabulary).ok()?;
                vec![ManaOutput::Counted {
                    mana_type: Some(mt),
                    permanents,
                }]
            }
            ManaSpecAst::CommanderColorIdentity => {
                vec![ManaOutput::SpecialColor(
                    SpecialColorYield::CommanderColorIdentity,
                )]
            }
            ManaSpecAst::AnyLandCouldProduce {
                controller: ControllerRelation::Opponent | ControllerRelation::Opponents,
            } => vec![ManaOutput::SpecialColor(
                SpecialColorYield::OpponentsLandColors,
            )],
            _ => return None,
        }
    };

    if base_outputs.len() > 1
        && act_restriction
            .as_ref()
            .is_some_and(|r| r.has_turn_activation_cap() || r.has_exhaust())
    {
        return None;
    }

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
    let mut final_outputs = Vec::with_capacity(base_outputs.len());
    for mut output in base_outputs {
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
                ManaOutput::Dynamic(
                    spellwake_engine::mana::DynamicManaYield::GreatestCreaturePower
                ) | ManaOutput::SpecialColor(SpecialColorYield::CommanderColorIdentity)
            )
        {
            return None;
        }

        if let Some(res) = restriction.clone() {
            output = output.with_modifier(ManaOutputModifier::Restrict(res));
        }
        if let Some(se) = spend_effect {
            output = output.with_modifier(ManaOutputModifier::SpendEffect(se));
        }
        if let Some(se) = side_effect.clone() {
            output = output.with_modifier(ManaOutputModifier::SideEffect(se));
        }
        if let Some(mc) = pay_mana.clone() {
            output = output.paid(mc);
        }
        if sac_self {
            if !valid_sacrifice_inner(&output) {
                return None;
            }
            output =
                output.with_modifier(ManaOutputModifier::Sacrifice(ManaSacrificeCost::Source {
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
        if let Some(cond) = act_restriction.clone() {
            output = output.with_activation_condition(cond);
        }
        final_outputs.push(output);
    }
    Some(final_outputs)
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
            interactive::parse_predicate_mana_restriction(raw, vocabulary)
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
            ValueExprAst::ChosenX => Ok(ExtraActivationCost::PayLife(LifeCostAmount::ChosenX)),
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
            let ck = counter_kind.trim().to_ascii_lowercase();
            let bare_count_word: Option<Option<NonZeroU8>> =
                if !*all && matches!(count.as_deref(), None | Some(ValueExprAst::Const(1))) {
                    match ck.as_str() {
                        "two" => Some(NonZeroU8::new(2)),
                        "three" => Some(NonZeroU8::new(3)),
                        "four" => Some(NonZeroU8::new(4)),
                        "five" => Some(NonZeroU8::new(5)),
                        "six" => Some(NonZeroU8::new(6)),
                        "seven" => Some(NonZeroU8::new(7)),
                        "eight" => Some(NonZeroU8::new(8)),
                        "nine" => Some(NonZeroU8::new(9)),
                        "ten" => Some(NonZeroU8::new(10)),
                        "x" => Some(None),
                        _ => None,
                    }
                } else {
                    None
                };
            let is_generic_kind =
                ck.is_empty() || matches!(ck.as_str(), "a" | "an" | "any" | "counter" | "counters");
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) {
                if is_generic_kind || bare_count_word.is_some() {
                    let cnt = if *all {
                        None
                    } else if let Some(wc) = bare_count_word {
                        wc
                    } else if let Some(c) = count {
                        opt_count(c)?
                    } else {
                        Some(NonZeroU8::MIN)
                    };
                    if *all || cnt == Some(NonZeroU8::MIN) {
                        return Ok(ExtraActivationCost::RemovePermanentCounters {
                            kind: None,
                            count: cnt,
                            all: *all,
                            filter: ObjectFilter::self_object(),
                        });
                    }
                    return Ok(ExtraActivationCost::RemoveCountersAmong {
                        kind: None,
                        count: cnt,
                        filter: ObjectFilter::self_object(),
                    });
                }
                let kind = parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                let delta = if *all {
                    CounterCostDelta::RemoveAll
                } else if let Some(c) = count {
                    match c.as_ref() {
                        ValueExprAst::Const(n) if *n > 0 => CounterCostDelta::Remove(nz_u8(*n)?),
                        ValueExprAst::ChosenX => CounterCostDelta::RemoveX,
                        _ => return Err("oracle:unsupported-activation-cost"),
                    }
                } else {
                    return Err("oracle:unsupported-activation-cost");
                };
                return Ok(ExtraActivationCost::SourceCounter { kind, delta });
            }
            let kind = if is_generic_kind {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?)
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
        CostAtomAst::RemoveCountersFromAmong {
            all: false,
            min_one: false,
            count,
            counter_kind,
            filter,
        } => {
            let ck = counter_kind.trim().to_ascii_lowercase();
            let bare_count_word: Option<Option<NonZeroU8>> =
                if matches!(count.as_deref(), None | Some(ValueExprAst::Const(1))) {
                    match ck.as_str() {
                        "two" => Some(NonZeroU8::new(2)),
                        "three" => Some(NonZeroU8::new(3)),
                        "four" => Some(NonZeroU8::new(4)),
                        "five" => Some(NonZeroU8::new(5)),
                        "six" => Some(NonZeroU8::new(6)),
                        "seven" => Some(NonZeroU8::new(7)),
                        "eight" => Some(NonZeroU8::new(8)),
                        "nine" => Some(NonZeroU8::new(9)),
                        "ten" => Some(NonZeroU8::new(10)),
                        "x" => Some(None),
                        _ => None,
                    }
                } else {
                    None
                };
            let (kind, cnt) = if let Some(wc) = bare_count_word {
                (None, wc)
            } else {
                let kind = if ck.is_empty()
                    || matches!(ck.as_str(), "a" | "an" | "any" | "counter" | "counters")
                {
                    None
                } else {
                    let parsed =
                        parse_counter_kind(counter_kind).ok_or("oracle:act-counter-kind")?;
                    if !parsed.is_permanent() {
                        return Err("oracle:act-counter-kind");
                    }
                    Some(parsed)
                };
                let cnt = match count {
                    Some(c) => opt_count(c)?,
                    None => Some(NonZeroU8::MIN),
                };
                (kind, cnt)
            };
            Ok(ExtraActivationCost::RemoveCountersAmong {
                kind,
                count: cnt,
                filter: lower_filter_any(filter)?,
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
        } => {
            let filter = strip_matching_chooser(filter, PlayerRefAst::You);
            Ok(ExtraActivationCost::SacrificePermanent {
                filter: lower_filter_any(&filter)?,
                count: opt_count(count)?,
            })
        }
        CostAtomAst::SacrificeAnyNumber(filter) => {
            let filter = strip_matching_chooser(filter, PlayerRefAst::You);
            Ok(ExtraActivationCost::SacrificeAnyNumber(lower_filter_any(
                &filter,
            )?))
        }
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
        CostAtomAst::BounceToHand(subject) => {
            let inner = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } => inner.as_ref(),
                other => other,
            };
            match inner {
                SubjectRefAst::SelfObject => Ok(ExtraActivationCost::ReturnSourceToHand),
                SubjectRefAst::Anaphora(a)
                    if a.trim().eq_ignore_ascii_case("it") || is_self_anaphora_label(a) =>
                {
                    Ok(ExtraActivationCost::ReturnSourceToHand)
                }
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
            }
        }
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
                    zone: ZoneFilter::YourHand | ZoneFilter::AnyHand,
                } => inner.as_ref(),
                other => other,
            };
            match inner {
                SubjectRefAst::SelfObject => Ok(ExtraActivationCost::RevealSource),
                SubjectRefAst::Anaphora(a) if is_self_anaphora_label(a) => {
                    Ok(ExtraActivationCost::RevealSource)
                }
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
        CostAtomAst::ExileTopLibrary { count, face_down } => {
            Ok(ExtraActivationCost::ExileTopLibrary {
                count: opt_count(count)?,
                face_down: *face_down,
            })
        }
        CostAtomAst::Exile {
            count: _,
            filter,
            face_down,
            face_up: _,
            single_graveyard: false,
            any_number,
            min_one,
            total_mv: None,
        } if *any_number || *min_one => {
            let zone = match filter.zone {
                Some(ZoneFilter::YourGraveyard) => CostExileZone::Graveyard,
                Some(ZoneFilter::YourHand) => CostExileZone::Hand,
                None | Some(ZoneFilter::Battlefield) => CostExileZone::Battlefield,
                _ => return Err("oracle:unsupported-activation-cost"),
            };
            Ok(ExtraActivationCost::ExileAnyNumber {
                zone,
                filter: lower_filter_any(filter)?,
                min_one: *min_one,
                face_down: *face_down,
            })
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
        CostAtomAst::PayManaChoice(choices) if choices.len() >= 2 => {
            let mut iter = choices.iter();
            let first_raw = iter.next().ok_or("oracle:unsupported-activation-cost")?;
            let mut acc =
                lower_extra_cost_atom(&CostAtomAst::PayMana(first_raw.clone()), vocabulary)?;
            for raw in iter {
                let next_cost =
                    lower_extra_cost_atom(&CostAtomAst::PayMana(raw.clone()), vocabulary)?;
                acc = ExtraActivationCost::Either(Box::new(acc), Box::new(next_cost));
            }
            Ok(acc)
        }
        CostAtomAst::PayMana(raw) => {
            let (mc, 0, hybs, snow) =
                parse_mana_cost_with_snow(raw).ok_or("oracle:unsupported-activation-cost")?
            else {
                return Err("oracle:unsupported-activation-cost");
            };
            if !hybs.is_empty() {
                return Err("oracle:unsupported-activation-cost");
            }
            if snow == 0 {
                Ok(ExtraActivationCost::PayMana(mc))
            } else if !mc.total().is_positive() {
                let nz = NonZeroU8::new(snow).ok_or("oracle:unsupported-activation-cost")?;
                Ok(ExtraActivationCost::PaySnowMana(nz))
            } else {
                Err("oracle:unsupported-activation-cost")
            }
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

fn extra_cost_requires_battlefield(cost: &ExtraActivationCost) -> bool {
    match cost {
        ExtraActivationCost::TapSource
        | ExtraActivationCost::UntapSource
        | ExtraActivationCost::SacrificeSource
        | ExtraActivationCost::ReturnSourceToHand
        | ExtraActivationCost::SourceCounter { .. }
        | ExtraActivationCost::ExertSource
        | ExtraActivationCost::ExileSource {
            from_graveyard: false,
            from_hand: false,
            ..
        }
        | ExtraActivationCost::Unattach { self_source: true } => true,
        ExtraActivationCost::Either(a, b) => {
            extra_cost_requires_battlefield(a) || extra_cost_requires_battlefield(b)
        }
        _ => false,
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
    let mut snow_mana: u8 = 0;
    let mut extra = Vec::new();
    for atom in cost {
        match atom {
            CostAtomAst::PayMana(raw) => {
                let (mc, x, hybs, snow) =
                    parse_mana_cost_with_snow(raw).ok_or("oracle:act-mana-cost")?;
                mana_cost.generic = mana_cost.generic.plus(&mc.generic);
                for (k, v) in mc.required {
                    let entry = mana_cost.required.entry(k).or_insert_with(|| 0.into());
                    *entry = entry.plus(&v);
                }
                hybrids.extend(hybs);
                x_symbols = x_symbols.checked_add(x).ok_or("oracle:x-ovf")?;
                snow_mana = snow_mana.checked_add(snow).ok_or("oracle:act-mana-cost")?;
            }
            other => {
                extra.push(lower_extra_cost_atom(other, vocabulary)?);
            }
        }
    }
    if let Some(nz_snow) = NonZeroU8::new(snow_mana) {
        extra.push(ExtraActivationCost::PaySnowMana(nz_snow));
    }
    if zone == Some("hand") && !extra.iter().any(ExtraActivationCost::is_from_hand) {
        return Err("oracle:unsupported-hand-activation");
    }
    if zone == Some("graveyard")
        && (extra.iter().any(extra_cost_requires_battlefield)
            || extra.iter().any(ExtraActivationCost::is_from_hand))
    {
        return Err("oracle:gy-tap-or-sac");
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

#[derive(Clone, Copy, Default)]
struct ActivationCostMeta {
    requires_x_nonzero: bool,
    rewrites_counter_prior_to_x: bool,
}

fn strip_or_more_counter_kind(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.len() >= 8 && trimmed[..8].eq_ignore_ascii_case("or more ") {
        Some(trimmed[8..].trim())
    } else if trimmed.eq_ignore_ascii_case("or more") {
        Some("")
    } else {
        None
    }
}

fn cost_atom_consumes_x(atom: &CostAtomAst) -> bool {
    match atom {
        CostAtomAst::PayMana(raw) => raw.contains("{X}") || raw.contains("{x}"),
        CostAtomAst::PayManaChoice(choices) => choices
            .iter()
            .any(|raw| raw.contains("{X}") || raw.contains("{x}")),
        CostAtomAst::PayLife(expr)
        | CostAtomAst::PayEnergyExpr(expr)
        | CostAtomAst::Mill(expr)
        | CostAtomAst::CollectEvidence(expr)
        | CostAtomAst::Blight(expr) => matches!(expr.as_ref(), ValueExprAst::ChosenX),
        CostAtomAst::Sacrifice { count, min_one, .. } => {
            *min_one || matches!(count.as_ref(), ValueExprAst::ChosenX)
        }
        CostAtomAst::SacrificeAnyNumber(_) => true,
        CostAtomAst::Discard {
            count, any_number, ..
        } => *any_number || matches!(count.as_ref(), ValueExprAst::ChosenX),
        CostAtomAst::Exile {
            count,
            any_number,
            min_one,
            ..
        } => *any_number || *min_one || matches!(count.as_ref(), ValueExprAst::ChosenX),
        CostAtomAst::ExileTopLibrary { count, .. } | CostAtomAst::ExileTopZone { count, .. } => {
            matches!(count.as_ref(), ValueExprAst::ChosenX)
        }
        CostAtomAst::TapPermanent {
            count, any_number, ..
        } => *any_number || matches!(count.as_ref(), ValueExprAst::ChosenX),
        CostAtomAst::UntapPermanent { count, .. } | CostAtomAst::PutCounters { count, .. } => {
            matches!(count.as_ref(), ValueExprAst::ChosenX)
        }
        CostAtomAst::RemoveCounters {
            all: false,
            count,
            counter_kind,
            ..
        } => {
            matches!(count.as_deref(), Some(ValueExprAst::ChosenX))
                || (matches!(count.as_deref(), None | Some(ValueExprAst::Const(1)))
                    && (counter_kind.trim().eq_ignore_ascii_case("x")
                        || strip_or_more_counter_kind(counter_kind).is_some()))
        }
        CostAtomAst::RemoveCountersFromAmong {
            all: false,
            min_one,
            count,
            counter_kind,
            ..
        } => {
            *min_one
                || matches!(count.as_deref(), Some(ValueExprAst::ChosenX))
                || (matches!(count.as_deref(), None | Some(ValueExprAst::Const(1)))
                    && counter_kind.trim().eq_ignore_ascii_case("x"))
        }
        CostAtomAst::Or(a, b) => cost_atom_consumes_x(a) || cost_atom_consumes_x(b),
        _ => false,
    }
}

fn lower_activation_cost_for_ability(
    cost: &[CostAtomAst],
    zone: Option<&str>,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(LoweredActivationCost, ActivationCostMeta), &'static str> {
    let mut has_min_one = false;
    let mut rewrites_counter_prior_to_x = false;
    let mut min_one_sacrifice = false;
    for atom in cost {
        match atom {
            CostAtomAst::RemoveCountersFromAmong {
                all: false,
                min_one: true,
                count: None,
                ..
            } => {
                has_min_one = true;
                rewrites_counter_prior_to_x = true;
            }
            CostAtomAst::RemoveCounters {
                all: false,
                count,
                counter_kind,
                ..
            } if matches!(count.as_deref(), None | Some(ValueExprAst::Const(1)))
                && strip_or_more_counter_kind(counter_kind).is_some() =>
            {
                has_min_one = true;
                rewrites_counter_prior_to_x = true;
            }
            CostAtomAst::Sacrifice {
                up_to: false,
                min_one: true,
                ..
            } => {
                has_min_one = true;
                min_one_sacrifice = true;
            }
            _ => {}
        }
    }
    if !has_min_one {
        return Ok((
            lower_activation_cost_full(cost, zone, vocabulary)?,
            ActivationCostMeta::default(),
        ));
    }
    // Fail closed if more than one cost atom consumes X to avoid binding collisions.
    if cost.iter().filter(|a| cost_atom_consumes_x(a)).count() != 1 {
        return Err("oracle:unsupported-activation-cost");
    }
    let mut normalized = Vec::with_capacity(cost.len());
    for atom in cost {
        match atom {
            CostAtomAst::RemoveCountersFromAmong {
                all: false,
                min_one: true,
                count: None,
                counter_kind,
                filter,
            } => {
                normalized.push(CostAtomAst::RemoveCountersFromAmong {
                    all: false,
                    min_one: false,
                    count: Some(Box::new(ValueExprAst::ChosenX)),
                    counter_kind: counter_kind.clone(),
                    filter: filter.clone(),
                });
            }
            CostAtomAst::RemoveCounters {
                all: false,
                count,
                counter_kind,
                subject,
            } if matches!(count.as_deref(), None | Some(ValueExprAst::Const(1)))
                && strip_or_more_counter_kind(counter_kind).is_some() =>
            {
                let real_kind = strip_or_more_counter_kind(counter_kind).unwrap_or("");
                normalized.push(CostAtomAst::RemoveCounters {
                    all: false,
                    count: Some(Box::new(ValueExprAst::ChosenX)),
                    counter_kind: if real_kind.is_empty() {
                        "any".to_string()
                    } else {
                        real_kind.to_string()
                    },
                    subject: subject.clone(),
                });
            }
            CostAtomAst::Sacrifice {
                filter,
                up_to: false,
                min_one: true,
                ..
            } => {
                normalized.push(CostAtomAst::SacrificeAnyNumber(filter.clone()));
            }
            other => normalized.push(other.clone()),
        }
    }
    let lowered = lower_activation_cost_full(&normalized, zone, vocabulary)?;
    if min_one_sacrifice {
        let (_, ref sc, _, ref extras) = lowered;
        let selected_extras = extras.iter().filter(|c| c.uses_selected_objects()).count();
        if selected_extras != 1 || sc.uses_selected_cost_origin() {
            return Err("oracle:unsupported-activation-cost");
        }
    }
    Ok((
        lowered,
        ActivationCostMeta {
            requires_x_nonzero: true,
            rewrites_counter_prior_to_x,
        },
    ))
}

fn replace_prior_counter_expr(expr: &mut ValueExprAst) {
    if matches!(
        expr,
        ValueExprAst::PriorResult | ValueExprAst::CountersRemovedThisWay
    ) {
        *expr = ValueExprAst::ChosenX;
    }
}

fn replace_prior_counter_in_subject(subject: &mut SubjectRefAst) {
    match subject {
        SubjectRefAst::TargetObject { filter, .. }
        | SubjectRefAst::Group(filter)
        | SubjectRefAst::All(filter)
        | SubjectRefAst::Each(filter) => {
            for bound in &mut filter.stat_bounds {
                replace_prior_counter_expr(bound.value.as_mut());
            }
        }
        SubjectRefAst::InZone { subject, .. } => replace_prior_counter_in_subject(subject),
        SubjectRefAst::Compound(items) => {
            for item in items {
                replace_prior_counter_in_subject(item);
            }
        }
        _ => {}
    }
}

fn rewrite_cost_counter_prior_to_chosen_x(ops: &mut Vec<InstructionAst>) {
    if ops.len() == 1
        && let InstructionAst::BindX { x_expr, .. } = &ops[0]
        && matches!(
            x_expr.as_ref(),
            ValueExprAst::PriorResult | ValueExprAst::CountersRemovedThisWay
        )
        && let Some(InstructionAst::BindX { body, .. }) = ops.pop()
    {
        *ops = body;
    }
    let Some(first) = ops.first_mut() else {
        return;
    };
    match first {
        InstructionAst::GainControl { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::MoveZone { subject, .. } => {
            replace_prior_counter_in_subject(subject);
        }
        InstructionAst::DealDamage { target, amount, .. } => {
            replace_prior_counter_in_subject(target);
            replace_prior_counter_expr(amount);
        }
        InstructionAst::AddMana {
            mana:
                ManaSpecAst::ScaledSymbol { amount, .. }
                | ManaSpecAst::AnyColor { amount, .. }
                | ManaSpecAst::DifferentColors { amount }
                | ManaSpecAst::CombinationOf { amount, .. },
            ..
        } => {
            replace_prior_counter_expr(amount);
        }
        InstructionAst::Draw { count, .. }
        | InstructionAst::Scry { count, .. }
        | InstructionAst::Surveil { count, .. }
        | InstructionAst::Mill { count, .. } => {
            replace_prior_counter_expr(count);
        }
        InstructionAst::ChangeLife { delta, .. } => {
            replace_prior_counter_expr(delta);
        }
        InstructionAst::PutCounters { subject, count, .. } => {
            replace_prior_counter_in_subject(subject);
            replace_prior_counter_expr(count);
        }
        InstructionAst::PreventDamage { amount, target, .. } => {
            replace_prior_counter_in_subject(target);
            replace_prior_counter_expr(amount);
        }
        InstructionAst::CreateToken { token, .. } => match token {
            TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. }
            | TokenSpecAst::Creature { count, .. } => {
                replace_prior_counter_expr(count);
            }
            TokenSpecAst::Role { .. } => {}
        },
        InstructionAst::ModifyPT { subject, scale, .. } => {
            replace_prior_counter_in_subject(subject);
            if let Some(sc) = scale {
                replace_prior_counter_expr(sc);
            }
        }
        _ => {}
    }
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
                let filter = strip_matching_chooser(filter, PlayerRefAst::You);
                let ssf = lower_selected_sacrifice_filter(&filter, vocabulary)?;
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
        if sac_self || pay_life.is_some() || sac_selected.is_some() || (from_gy && tap_self) {
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
        if sac_self || (from_gy && tap_self) {
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

fn combat_steps_condition() -> TriggerCondition {
    TriggerCondition::Or(
        Box::new(TriggerCondition::DuringStep(Step::BeginningCombat)),
        Box::new(TriggerCondition::Or(
            Box::new(TriggerCondition::DuringStep(Step::DeclareAttackers)),
            Box::new(TriggerCondition::Or(
                Box::new(TriggerCondition::DuringStep(Step::DeclareBlockers)),
                Box::new(TriggerCondition::Or(
                    Box::new(TriggerCondition::DuringStep(Step::CombatDamage)),
                    Box::new(TriggerCondition::DuringStep(Step::EndCombat)),
                )),
            )),
        )),
    )
}

struct UnwrappedQualifiedTrigger<'a> {
    inner: &'a TriggerEventAst,
    first_time_each_turn: bool,
    condition: Option<TriggerCondition>,
    enters_tapped: Option<bool>,
    while_exiled: bool,
}

fn apply_enters_tapped_filter(
    event: &mut TriggerEvent,
    expected: bool,
) -> Result<(), &'static str> {
    let apply_clause = |clause: &mut ObjectClause| -> Result<(), &'static str> {
        if clause.tapped == Some(!expected) {
            return Err("oracle:conflicting-tapped-filter");
        }
        clause.tapped = Some(expected);
        Ok(())
    };
    match event {
        TriggerEvent::Enters { subject, .. } => match subject {
            ObjectFilter::Clause(c) => apply_clause(c),
            ObjectFilter::Either(c1, c2) => {
                apply_clause(c1)?;
                apply_clause(c2)
            }
        },
        TriggerEvent::AnyOf(events) if !events.is_empty() => {
            for ev in events {
                apply_enters_tapped_filter(ev, expected)?;
            }
            Ok(())
        }
        _ => Err("oracle:unsupported-trigger-qualifier"),
    }
}

fn unwrap_qualified_trigger(
    mut ev: &TriggerEventAst,
    has_intervening_if: bool,
) -> Result<UnwrappedQualifiedTrigger<'_>, &'static str> {
    let mut first_time_each_turn = false;
    let mut timings: Vec<&str> = Vec::new();
    while let TriggerEventAst::QualifiedTrigger {
        inner,
        first_time_each_turn: ft,
        timing,
    } = ev
    {
        if *ft {
            first_time_each_turn = true;
        }
        if let Some(t) = timing.as_deref() {
            timings.push(t);
        }
        ev = inner.as_ref();
    }
    // CR 603.2g: OneOrMoreOnceEachTurn tracks whether the ability triggered, which is exact
    // for "for the first time each turn" only when there is no intervening-if clause.
    if first_time_each_turn && has_intervening_if {
        return Err("oracle:unsupported-trigger-qualifier");
    }
    let mut cond: Option<TriggerCondition> = None;
    let mut push_cond = |next: TriggerCondition| {
        cond = Some(match cond.take() {
            Some(prev) => TriggerCondition::And(Box::new(prev), Box::new(next)),
            None => next,
        });
    };
    let mut enters_tapped: Option<bool> = None;
    let mut set_tapped = |expected: bool| -> Result<(), &'static str> {
        if enters_tapped == Some(!expected) {
            return Err("oracle:unsupported-trigger-qualifier");
        }
        enters_tapped = Some(expected);
        Ok(())
    };
    let mut while_exiled = false;
    for t in timings {
        let low = t.trim().to_ascii_lowercase();
        let (timing_part, tapped_part) = if let Some((prefix, suffix)) = low.split_once(':') {
            (prefix.trim(), Some(suffix.trim()))
        } else {
            (low.as_str(), None)
        };
        if let Some(tap_tag) = tapped_part {
            match timing_part {
                "during_combat" => push_cond(combat_steps_condition()),
                "during_declare_attackers_step" => {
                    push_cond(TriggerCondition::DuringStep(Step::DeclareAttackers));
                }
                _ => return Err("oracle:unsupported-trigger-qualifier"),
            }
            match tap_tag {
                "tapped" => set_tapped(true)?,
                "untapped" => set_tapped(false)?,
                _ => return Err("oracle:unsupported-trigger-qualifier"),
            }
            continue;
        }
        match timing_part {
            "during_your_turn" => push_cond(TriggerCondition::ControllersTurn),
            "during_opponents_turn" | "during_an_opponents_turn" | "during_each_opponents_turn" => {
                push_cond(TriggerCondition::OpponentsTurn)
            }
            "during_their_turn" => {
                if matches!(
                    ev,
                    TriggerEventAst::SpellCast { .. }
                        | TriggerEventAst::CardDrawn { .. }
                        | TriggerEventAst::LifeChanged { .. }
                        | TriggerEventAst::LandPlayed { .. }
                        | TriggerEventAst::CardPlayed { .. }
                        | TriggerEventAst::CardDiscarded { .. }
                        | TriggerEventAst::CardMilled { .. }
                        | TriggerEventAst::PermanentSacrificed { .. }
                        | TriggerEventAst::AbilityActivated { .. }
                        | TriggerEventAst::QualifiedAbilityActivated { .. }
                        | TriggerEventAst::CrimeCommitted { .. }
                        | TriggerEventAst::Attacks { .. }
                        | TriggerEventAst::DamageDealt { .. }
                        | TriggerEventAst::TapStateChanged { .. }
                        | TriggerEventAst::ZoneChange { .. }
                        | TriggerEventAst::KeywordActionPerformed { .. }
                ) {
                    push_cond(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(
                            ResolvingPlayer::UniqueRelatedPlayer,
                        )),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })));
                } else {
                    return Err("oracle:unsupported-trigger-qualifier");
                }
            }
            "during_your_main_phase" => {
                push_cond(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(TriggerCondition::Or(
                        Box::new(TriggerCondition::DuringStep(Step::PrecombatMain)),
                        Box::new(TriggerCondition::DuringStep(Step::PostcombatMain)),
                    )),
                ));
            }
            "during_main_phase" => {
                push_cond(TriggerCondition::Or(
                    Box::new(TriggerCondition::DuringStep(Step::PrecombatMain)),
                    Box::new(TriggerCondition::DuringStep(Step::PostcombatMain)),
                ));
            }
            "during_your_upkeep" => {
                push_cond(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(TriggerCondition::DuringStep(Step::Upkeep)),
                ));
            }
            "during_your_end_step" => {
                push_cond(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(TriggerCondition::DuringStep(Step::End)),
                ));
            }
            "during_your_draw_step" => {
                push_cond(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(TriggerCondition::DuringStep(Step::Draw)),
                ));
            }
            "during_your_combat" | "during_combat_on_your_turn" => {
                push_cond(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(combat_steps_condition()),
                ));
            }
            "during_combat" => push_cond(combat_steps_condition()),
            "during_declare_attackers_step" => {
                push_cond(TriggerCondition::DuringStep(Step::DeclareAttackers));
            }
            "enters_tapped" => set_tapped(true)?,
            "enters_untapped" => set_tapped(false)?,
            "while_exiled" => {
                if matches!(ev, TriggerEventAst::CountersChanged { .. }) {
                    while_exiled = true;
                } else {
                    return Err("oracle:unsupported-trigger-qualifier");
                }
            }
            "other_than_first_each_turn" => {
                if let TriggerEventAst::SpellCast {
                    caster,
                    spell,
                    self_spell: false,
                    targets: None,
                    nth_each_turn: None,
                    include_copy: false,
                } = ev
                    && spell.as_deref().is_none_or(is_unrestricted_filter)
                {
                    match caster {
                        PlayerRefAst::You => {
                            push_cond(TriggerCondition::ControllerCastSpellsThisTurn(2));
                        }
                        PlayerRefAst::AnyOpponent
                        | PlayerRefAst::EachOpponent
                        | PlayerRefAst::AnyPlayer
                        | PlayerRefAst::EachPlayer => {
                            push_cond(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                left: ValueExpr::PlayerStat(
                                    PlayerStatExpr::SpellsCastMatchingThisTurn {
                                        players: AffectedPlayers::One(
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                        ),
                                        filter: Box::new(ObjectFilter::any()),
                                        from_zone: None,
                                    },
                                ),
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(2),
                            })));
                        }
                        _ => return Err("oracle:unsupported-trigger-qualifier"),
                    }
                } else {
                    return Err("oracle:unsupported-trigger-qualifier");
                }
            }
            _ => return Err("oracle:unsupported-trigger-qualifier"),
        }
    }
    Ok(UnwrappedQualifiedTrigger {
        inner: ev,
        first_time_each_turn,
        condition: cond,
        enters_tapped,
        while_exiled,
    })
}

pub(super) fn lower_trigger_spec(
    trig: &TriggerSpecAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(TriggerSpec, TriggerGrouping, Option<InterveningCondition>), &'static str> {
    if trig.events.is_empty() {
        return Err("oracle:empty-trigger");
    }
    let has_intervening_if = trig.intervening_if.is_some();
    let mut unwrapped_events = Vec::with_capacity(trig.events.len());
    let mut qualified_first_time = false;
    let mut qualified_cond: Option<TriggerCondition> = None;
    let mut qualified_enters_tapped: Option<bool> = None;
    let mut qualified_while_exiled = false;
    for (idx, ev) in trig.events.iter().enumerate() {
        let unwrapped = unwrap_qualified_trigger(ev, has_intervening_if)?;
        if idx == 0 {
            qualified_first_time = unwrapped.first_time_each_turn;
            qualified_cond = unwrapped.condition;
            qualified_enters_tapped = unwrapped.enters_tapped;
            qualified_while_exiled = unwrapped.while_exiled;
        } else if unwrapped.first_time_each_turn != qualified_first_time
            || unwrapped.condition != qualified_cond
            || unwrapped.enters_tapped != qualified_enters_tapped
            || unwrapped.while_exiled != qualified_while_exiled
        {
            return Err("oracle:conflicting-trigger-qualifiers");
        }
        unwrapped_events.push(unwrapped.inner);
    }
    let mut grouping = TriggerGrouping::EachOccurrence;
    let mut events = Vec::with_capacity(unwrapped_events.len());
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
    if unwrapped_events.len() == 1 {
        match unwrapped_events[0] {
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
                        "an opponent loses life for the first time during each of their turns" => {
                            events.push(TriggerEvent::LifeChanged {
                                kind: LifeChangeKind::Loss,
                                player: PlayerRelation::Opponent,
                            });
                            grouping = TriggerGrouping::OneOrMoreOnceEachTurn;
                            extra_cond =
                                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::PlayerStat(
                                        PlayerStatExpr::PlayerIsActivePlayer(
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                        ),
                                    ),
                                    comparison: Comparison::Equal,
                                    right: ValueExpr::literal(1),
                                })));
                            handled_single = true;
                        }
                        "you cast an instant or sorcery spell during your main phase" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::from_object(ObjectFilter::Clause(
                                    ObjectClause::any().any_types(
                                        CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
                                    ),
                                )),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(own_main_phase());
                            handled_single = true;
                        }
                        "you cast a creature spell during your main phase" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::from_object(ObjectFilter::creature()),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(own_main_phase());
                            handled_single = true;
                        }
                        "you cast a noncreature spell during your main phase" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::from_object(ObjectFilter::Clause(
                                    ObjectClause::any().noncreature(),
                                )),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(own_main_phase());
                            handled_single = true;
                        }
                        "you cast a spell during an opponent's turn" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(TriggerCondition::OpponentsTurn);
                            handled_single = true;
                        }
                        "an opponent casts a spell during your turn" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(TriggerCondition::ControllersTurn);
                            handled_single = true;
                        }
                        "an opponent casts a spell during their turn" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond =
                                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::PlayerStat(
                                        PlayerStatExpr::PlayerIsActivePlayer(
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                        ),
                                    ),
                                    comparison: Comparison::Equal,
                                    right: ValueExpr::literal(1),
                                })));
                            handled_single = true;
                        }
                        "a player casts a spell during their turn" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(
                                    PlayerRelation::AnyParticipant,
                                ),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond =
                                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::PlayerStat(
                                        PlayerStatExpr::PlayerIsActivePlayer(
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                        ),
                                    ),
                                    comparison: Comparison::Equal,
                                    right: ValueExpr::literal(1),
                                })));
                            handled_single = true;
                        }
                        "whenever an opponent who controls an artifact named ~ draws a card during their turn"
                        | "an opponent who controls an artifact named ~ draws a card during their turn" =>
                        {
                            events.push(TriggerEvent::PlayerAction {
                                player: PlayerRelation::Opponent,
                                action: PlayerTriggerAction::DrewCard {
                                    nth_each_turn: None,
                                },
                            });
                            extra_cond = Some(TriggerCondition::And(
                                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::PlayerStat(
                                        PlayerStatExpr::PlayerIsActivePlayer(
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                        ),
                                    ),
                                    comparison: Comparison::Equal,
                                    right: ValueExpr::literal(1),
                                }))),
                                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::count_player_battlefield(
                                        ResolvingPlayer::UniqueRelatedPlayer,
                                        ObjectFilter::artifact().sharing_name_with_source(),
                                    ),
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(1),
                                }))),
                            ));
                            handled_single = true;
                        }
                        "whenever an opponent who controls an artifact named ~ gains life during their turn"
                        | "an opponent who controls an artifact named ~ gains life during their turn" =>
                        {
                            events.push(TriggerEvent::LifeChanged {
                                kind: LifeChangeKind::Gain,
                                player: PlayerRelation::Opponent,
                            });
                            extra_cond = Some(TriggerCondition::And(
                                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::PlayerStat(
                                        PlayerStatExpr::PlayerIsActivePlayer(
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                        ),
                                    ),
                                    comparison: Comparison::Equal,
                                    right: ValueExpr::literal(1),
                                }))),
                                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::count_player_battlefield(
                                        ResolvingPlayer::UniqueRelatedPlayer,
                                        ObjectFilter::artifact().sharing_name_with_source(),
                                    ),
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(1),
                                }))),
                            ));
                            handled_single = true;
                        }
                        "a player casts a spell they don't own" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(
                                    PlayerRelation::AnyParticipant,
                                ),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond =
                                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::ObjectMatches {
                                        subject: MatchObjectRef::TriggerSubject,
                                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                                            flags: ObjectFilterFlags::NOT_OWNED_BY_CONTROLLER,
                                            ..ObjectClause::any()
                                        })),
                                    },
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(1),
                                })));
                            handled_single = true;
                        }
                        "you cast a spell with power, toughness, or mana value 4" => {
                            events.push(TriggerEvent::SpellCast {
                                caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                                filter: SpellFilter::any(),
                                self_cast: false,
                                include_copy: false,
                                min_mana_value: None,
                            });
                            extra_cond = Some(TriggerCondition::Or(
                                Box::new(TriggerCondition::Or(
                                    Box::new(TriggerCondition::Eval(Box::new(
                                        ResolutionCondition {
                                            left: ValueExpr::ObjectStat {
                                                subject: ValueObjectRef::TriggerSubject,
                                                stat: ObjectStatKind::ManaValue,
                                            },
                                            comparison: Comparison::Equal,
                                            right: ValueExpr::literal(4),
                                        },
                                    ))),
                                    Box::new(TriggerCondition::Eval(Box::new(
                                        ResolutionCondition {
                                            left: ValueExpr::ObjectMatches {
                                                subject: MatchObjectRef::TriggerSubject,
                                                filter: Box::new(
                                                    ObjectFilter::creature()
                                                        .with_power(PowerFilter::Equal(4)),
                                                ),
                                            },
                                            comparison: Comparison::GreaterOrEqual,
                                            right: ValueExpr::literal(1),
                                        },
                                    ))),
                                )),
                                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                    left: ValueExpr::ObjectMatches {
                                        subject: MatchObjectRef::TriggerSubject,
                                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                                            toughness: ToughnessFilter::Equal(4),
                                            ..ObjectClause::creature()
                                        })),
                                    },
                                    comparison: Comparison::GreaterOrEqual,
                                    right: ValueExpr::literal(1),
                                }))),
                            ));
                            handled_single = true;
                        }
                        "you cast a spell that's exactly two colors" => {
                            let all_colors = [
                                Color::White,
                                Color::Blue,
                                Color::Black,
                                Color::Red,
                                Color::Green,
                            ];
                            let mut pair_cond: Option<TriggerCondition> = None;
                            for i in 0..all_colors.len() {
                                for j in (i + 1)..all_colors.len() {
                                    let c1 = all_colors[i];
                                    let c2 = all_colors[j];
                                    let mut none_of = ColorMask::EMPTY;
                                    for (k, &ck) in all_colors.iter().enumerate() {
                                        if k != i && k != j {
                                            none_of = none_of.union(ColorMask::from_color(ck));
                                        }
                                    }
                                    let has_c1_no_others =
                                        TriggerCondition::Eval(Box::new(ResolutionCondition {
                                            left: ValueExpr::ObjectMatches {
                                                subject: MatchObjectRef::TriggerSubject,
                                                filter: Box::new(ObjectFilter::any().with_color(
                                                    ObjectColorFilter::Mask {
                                                        any_of: ColorMask::from_color(c1),
                                                        none_of,
                                                    },
                                                )),
                                            },
                                            comparison: Comparison::GreaterOrEqual,
                                            right: ValueExpr::literal(1),
                                        }));
                                    let has_c2 =
                                        TriggerCondition::Eval(Box::new(ResolutionCondition {
                                            left: ValueExpr::ObjectMatches {
                                                subject: MatchObjectRef::TriggerSubject,
                                                filter: Box::new(
                                                    ObjectFilter::any()
                                                        .with_color(ObjectColorFilter::has(c2)),
                                                ),
                                            },
                                            comparison: Comparison::GreaterOrEqual,
                                            right: ValueExpr::literal(1),
                                        }));
                                    let exact_pair = TriggerCondition::And(
                                        Box::new(has_c1_no_others),
                                        Box::new(has_c2),
                                    );
                                    pair_cond = Some(match pair_cond {
                                        Some(prev) => TriggerCondition::Or(
                                            Box::new(prev),
                                            Box::new(exact_pair),
                                        ),
                                        None => exact_pair,
                                    });
                                }
                            }
                            if let Some(cond) = pair_cond {
                                events.push(TriggerEvent::SpellCast {
                                    caster: TriggerPlayerScope::Relation(
                                        PlayerRelation::Controller,
                                    ),
                                    filter: SpellFilter::any(),
                                    self_cast: false,
                                    include_copy: false,
                                    min_mana_value: None,
                                });
                                extra_cond = Some(cond);
                                handled_single = true;
                            }
                        }
                        "whenever ~ saddles a mount or crews a vehicle during your main phase"
                        | "~ saddles a mount or crews a vehicle during your main phase" => {
                            events.push(TriggerEvent::AnyOf(vec![
                                TriggerEvent::PlayerAction {
                                    player: PlayerRelation::Controller,
                                    action: PlayerTriggerAction::Saddled,
                                },
                                TriggerEvent::StatusChanged {
                                    subject: ObjectFilter::self_object(),
                                    kind: StatusTriggerKind::Crewed,
                                },
                            ]));
                            extra_cond = Some(own_main_phase());
                            handled_single = true;
                        }
                        _ => {
                            // Filtered or player-parameterized Nth spell triggers:
                            // "you cast your second <filter> spell each turn"
                            // "an opponent casts their second [<filter>] spell each turn"
                            // "a player casts their second [<filter>] spell each turn"
                            let nth_parts = low
                                .strip_prefix("you cast your ")
                                .map(|r| {
                                    (PlayerRelation::Controller, ResolvingPlayer::Controller, r)
                                })
                                .or_else(|| {
                                    low.strip_prefix("an opponent casts their ").map(|r| {
                                        (
                                            PlayerRelation::Opponent,
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                            r,
                                        )
                                    })
                                })
                                .or_else(|| {
                                    low.strip_prefix("a player casts their ").map(|r| {
                                        (
                                            PlayerRelation::AnyParticipant,
                                            ResolvingPlayer::UniqueRelatedPlayer,
                                            r,
                                        )
                                    })
                                });
                            if let Some((caster_rel, count_rp, after_who)) = nth_parts
                                && let Some(mid) = after_who
                                    .strip_suffix(" spell each turn")
                                    .or_else(|| after_who.strip_suffix(" spell in a turn"))
                            {
                                let (ord_word, flt_opt) = match mid.trim().split_once(' ') {
                                    Some((o, f)) => (o.trim(), Some(f.trim())),
                                    None => (mid.trim(), None),
                                };
                                let ord_n: Option<u32> = match ord_word {
                                    "first" | "1st" => Some(1),
                                    "second" | "2nd" => Some(2),
                                    "third" | "3rd" => Some(3),
                                    "fourth" | "4th" => Some(4),
                                    "fifth" | "5th" => Some(5),
                                    _ => None,
                                };
                                if let Some(n) = ord_n
                                    && let Some(nz) = NonZeroU32::new(n)
                                {
                                    match flt_opt {
                                        None => {
                                            events.push(TriggerEvent::SpellCast {
                                                caster: TriggerPlayerScope::Relation(caster_rel),
                                                filter: SpellFilter::any().ordinal_this_turn(nz),
                                                self_cast: false,
                                                include_copy: false,
                                                min_mana_value: None,
                                            });
                                            handled_single = true;
                                        }
                                        Some(flt_raw) => {
                                            let compiler_vocab = cached_compiler_vocab(vocabulary);
                                            let spell_phrase = format!("{flt_raw} spell");
                                            if let Ok(parsed) =
                                                spellwake_oracle_compiler::filter::parse_object_filter(
                                                    &spell_phrase,
                                                    &compiler_vocab,
                                                )
                                                && parsed.zone.is_none()
                                                && parsed.extra_zones.is_empty()
                                                && parsed.controller.is_none()
                                                && parsed.owner.is_none()
                                                && parsed.named.is_none()
                                                && parsed.predicates.is_empty()
                                                && parsed.flags.is_empty()
                                                && let Ok(obj) =
                                                    lower_object_filter(&parsed, vocabulary)
                                                && filter_supported_by_cast_spell_record(&obj)
                                                && let Ok(sf) =
                                                    lower_spell_filter(&parsed, vocabulary)
                                            {
                                                events.push(TriggerEvent::SpellCast {
                                                    caster: TriggerPlayerScope::Relation(
                                                        caster_rel,
                                                    ),
                                                    filter: sf,
                                                    self_cast: false,
                                                    include_copy: false,
                                                    min_mana_value: None,
                                                });
                                                extra_cond = Some(TriggerCondition::Eval(
                                                    Box::new(ResolutionCondition {
                                                        left: ValueExpr::PlayerStat(
                                                            PlayerStatExpr::SpellsCastMatchingThisTurn {
                                                                players: AffectedPlayers::One(
                                                                    count_rp,
                                                                ),
                                                                filter: Box::new(obj),
                                                                from_zone: None,
                                                            },
                                                        ),
                                                        comparison: Comparison::Equal,
                                                        right: ValueExpr::literal(i64::from(n)),
                                                    }),
                                                ));
                                                handled_single = true;
                                            }
                                        }
                                    }
                                }
                            }
                            if !handled_single
                                && let Some(ord_raw) =
                                    low.strip_prefix("you roll your ").and_then(|r| {
                                        r.strip_suffix(" die each turn")
                                            .or_else(|| r.strip_suffix(" die in a turn"))
                                    })
                                && let Some(ord_n) = match ord_raw.trim() {
                                    "first" | "1st" => Some(1i64),
                                    "second" | "2nd" => Some(2i64),
                                    "third" | "3rd" => Some(3i64),
                                    "fourth" | "4th" => Some(4i64),
                                    "fifth" | "5th" => Some(5i64),
                                    _ => None,
                                }
                            {
                                events.push(TriggerEvent::PlayerAction {
                                    player: PlayerRelation::Controller,
                                    action: PlayerTriggerAction::RolledDie { exact: None },
                                });
                                extra_cond =
                                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                                        left: ValueExpr::PlayerStat(
                                            PlayerStatExpr::DiceRolledThisTurn,
                                        ),
                                        comparison: Comparison::Equal,
                                        right: ValueExpr::literal(ord_n),
                                    })));
                                handled_single = true;
                            }
                            if !handled_single
                                && let Some(after_the) = low.strip_prefix("the ")
                                && let Some((ord_word, rest_after_ord)) = after_the.split_once(' ')
                                && let Some(ord_n) = match ord_word.trim() {
                                    "first" | "1st" => Some(1i64),
                                    "second" | "2nd" => Some(2i64),
                                    "third" | "3rd" => Some(3i64),
                                    "fourth" | "4th" => Some(4i64),
                                    "fifth" | "5th" => Some(5i64),
                                    _ => None,
                                }
                                && let Some(mid) = rest_after_ord
                                    .strip_suffix(" of a turn is cast")
                                    .or_else(|| rest_after_ord.strip_suffix(" of the turn is cast"))
                                    .or_else(|| rest_after_ord.strip_suffix(" each turn is cast"))
                                && mid.trim().ends_with("spell")
                            {
                                let spell_phrase = mid.trim();
                                let parsed_res = if spell_phrase == "spell" {
                                    Ok((ObjectFilter::any(), SpellFilter::any()))
                                } else {
                                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                                    spellwake_oracle_compiler::filter::parse_object_filter(
                                        spell_phrase,
                                        &compiler_vocab,
                                    )
                                    .ok()
                                    .filter(|p| {
                                        p.zone.is_none()
                                            && p.extra_zones.is_empty()
                                            && p.controller.is_none()
                                            && p.owner.is_none()
                                            && p.named.is_none()
                                            && p.predicates.is_empty()
                                            && p.flags.is_empty()
                                    })
                                    .and_then(|p| {
                                        let obj = lower_object_filter(&p, vocabulary).ok()?;
                                        if !filter_supported_by_cast_spell_record(&obj) {
                                            return None;
                                        }
                                        let sf = lower_spell_filter(&p, vocabulary).ok()?;
                                        Some((obj, sf))
                                    })
                                    .ok_or(())
                                };
                                if let Ok((obj, sf)) = parsed_res {
                                    events.push(TriggerEvent::SpellCast {
                                        caster: TriggerPlayerScope::Relation(
                                            PlayerRelation::AnyParticipant,
                                        ),
                                        filter: sf,
                                        self_cast: false,
                                        include_copy: false,
                                        min_mana_value: None,
                                    });
                                    extra_cond = Some(TriggerCondition::Eval(Box::new(
                                        ResolutionCondition {
                                            left: ValueExpr::PlayerStat(
                                                PlayerStatExpr::SpellsCastMatchingThisTurn {
                                                    players: AffectedPlayers::EachPlayer,
                                                    filter: Box::new(obj),
                                                    from_zone: None,
                                                },
                                            ),
                                            comparison: Comparison::Equal,
                                            right: ValueExpr::literal(ord_n),
                                        },
                                    )));
                                    handled_single = true;
                                }
                            }
                            if !handled_single
                                && let Some((lhs_raw, rhs_raw)) =
                                    low.split_once(" other than the first ")
                            {
                                let caster_info = lhs_raw
                                    .strip_prefix("you cast ")
                                    .map(|s| {
                                        (
                                            TriggerPlayerScope::Relation(
                                                PlayerRelation::Controller,
                                            ),
                                            ResolvingPlayer::Controller,
                                            s.trim(),
                                        )
                                    })
                                    .or_else(|| {
                                        lhs_raw.strip_prefix("an opponent casts ").map(|s| {
                                            (
                                                TriggerPlayerScope::Relation(
                                                    PlayerRelation::Opponent,
                                                ),
                                                ResolvingPlayer::UniqueRelatedPlayer,
                                                s.trim(),
                                            )
                                        })
                                    })
                                    .or_else(|| {
                                        lhs_raw.strip_prefix("a player casts ").map(|s| {
                                            (
                                                TriggerPlayerScope::Relation(
                                                    PlayerRelation::AnyParticipant,
                                                ),
                                                ResolvingPlayer::UniqueRelatedPlayer,
                                                s.trim(),
                                            )
                                        })
                                    })
                                    .or_else(|| {
                                        lhs_raw.strip_prefix("enchanted player casts ").map(|s| {
                                            (
                                                TriggerPlayerScope::EnchantedPlayer,
                                                ResolvingPlayer::EnchantedPlayer,
                                                s.trim(),
                                            )
                                        })
                                    });
                                let rhs_spell_raw = rhs_raw
                                    .strip_suffix(" you cast each turn")
                                    .or_else(|| rhs_raw.strip_suffix(" you cast that turn"))
                                    .or_else(|| rhs_raw.strip_suffix(" you cast in a turn"))
                                    .or_else(|| {
                                        rhs_raw.strip_suffix(" that player casts each turn")
                                    })
                                    .or_else(|| {
                                        rhs_raw.strip_suffix(" that player casts that turn")
                                    })
                                    .or_else(|| {
                                        rhs_raw.strip_suffix(" that player casts in a turn")
                                    })
                                    .or_else(|| rhs_raw.strip_suffix(" they cast each turn"))
                                    .or_else(|| rhs_raw.strip_suffix(" they cast that turn"))
                                    .or_else(|| rhs_raw.strip_suffix(" they cast in a turn"))
                                    .map(str::trim);
                                if let (Some((scope, count_rp, lhs_spell)), Some(rhs_spell)) =
                                    (caster_info, rhs_spell_raw)
                                {
                                    let norm_lhs = lhs_spell
                                        .strip_prefix("an ")
                                        .or_else(|| lhs_spell.strip_prefix("a "))
                                        .unwrap_or(lhs_spell)
                                        .trim();
                                    if norm_lhs == rhs_spell && norm_lhs.ends_with("spell") {
                                        let parsed_res = if norm_lhs == "spell" {
                                            Ok((ObjectFilter::any(), SpellFilter::any()))
                                        } else {
                                            let compiler_vocab = cached_compiler_vocab(vocabulary);
                                            spellwake_oracle_compiler::filter::parse_object_filter(
                                                norm_lhs,
                                                &compiler_vocab,
                                            )
                                            .ok()
                                            .filter(|p| {
                                                p.zone.is_none()
                                                    && p.extra_zones.is_empty()
                                                    && p.controller.is_none()
                                                    && p.owner.is_none()
                                                    && p.named.is_none()
                                                    && p.predicates.is_empty()
                                                    && p.flags.is_empty()
                                            })
                                            .and_then(|p| {
                                                let obj =
                                                    lower_object_filter(&p, vocabulary).ok()?;
                                                if !filter_supported_by_cast_spell_record(&obj) {
                                                    return None;
                                                }
                                                let sf = lower_spell_filter(&p, vocabulary).ok()?;
                                                Some((obj, sf))
                                            })
                                            .ok_or(())
                                        };
                                        if let Ok((obj, sf)) = parsed_res {
                                            events.push(TriggerEvent::SpellCast {
                                                caster: scope,
                                                filter: sf,
                                                self_cast: false,
                                                include_copy: false,
                                                min_mana_value: None,
                                            });
                                            extra_cond = Some(TriggerCondition::Eval(Box::new(
                                                ResolutionCondition {
                                                    left: ValueExpr::PlayerStat(
                                                        PlayerStatExpr::SpellsCastMatchingThisTurn {
                                                            players: AffectedPlayers::One(count_rp),
                                                            filter: Box::new(obj),
                                                            from_zone: None,
                                                        },
                                                    ),
                                                    comparison: Comparison::GreaterOrEqual,
                                                    right: ValueExpr::literal(2),
                                                },
                                            )));
                                            handled_single = true;
                                        }
                                    }
                                }
                            }
                            if !handled_single {
                                let timing_split = low
                                    .strip_suffix(" during your main phase")
                                    .map(|head| (head.trim(), own_main_phase()))
                                    .or_else(|| {
                                        low.strip_suffix(" during your turn")
                                            .map(|head| (head.trim(), TriggerCondition::ControllersTurn))
                                    })
                                    .or_else(|| {
                                        low.strip_suffix(" during an opponent's turn")
                                            .or_else(|| low.strip_suffix(" during each opponent's turn"))
                                            .map(|head| (head.trim(), TriggerCondition::OpponentsTurn))
                                    })
                                    .or_else(|| {
                                        low.strip_suffix(" during their turn")
                                            .or_else(|| low.strip_suffix(" during that player's turn"))
                                            .map(|head| {
                                                (
                                                    head.trim(),
                                                    TriggerCondition::Eval(Box::new(
                                                        ResolutionCondition {
                                                            left: ValueExpr::PlayerStat(
                                                                PlayerStatExpr::PlayerIsActivePlayer(
                                                                    ResolvingPlayer::UniqueRelatedPlayer,
                                                                ),
                                                            ),
                                                            comparison: Comparison::Equal,
                                                            right: ValueExpr::literal(1),
                                                        },
                                                    )),
                                                )
                                            })
                                    });
                                if let Some((head, turn_cond)) = timing_split {
                                    let cast_parts = head
                                        .strip_prefix("you cast ")
                                        .map(|s| {
                                            (
                                                TriggerPlayerScope::Relation(
                                                    PlayerRelation::Controller,
                                                ),
                                                s.trim(),
                                            )
                                        })
                                        .or_else(|| {
                                            head.strip_prefix("an opponent casts ").map(|s| {
                                                (
                                                    TriggerPlayerScope::Relation(
                                                        PlayerRelation::Opponent,
                                                    ),
                                                    s.trim(),
                                                )
                                            })
                                        })
                                        .or_else(|| {
                                            head.strip_prefix("a player casts ").map(|s| {
                                                (
                                                    TriggerPlayerScope::Relation(
                                                        PlayerRelation::AnyParticipant,
                                                    ),
                                                    s.trim(),
                                                )
                                            })
                                        })
                                        .or_else(|| {
                                            head.strip_suffix(" is cast").map(|s| {
                                                (
                                                    TriggerPlayerScope::Relation(
                                                        PlayerRelation::AnyParticipant,
                                                    ),
                                                    s.trim(),
                                                )
                                            })
                                        });
                                    if let Some((scope, spell_raw)) = cast_parts
                                        && !spell_raw.contains(" or activate ")
                                        && !spell_raw.contains(" from ")
                                        && !spell_raw.contains(" other than ")
                                        && spell_raw.ends_with("spell")
                                    {
                                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                                        if let Ok(parsed) =
                                            spellwake_oracle_compiler::filter::parse_object_filter(
                                                spell_raw,
                                                &compiler_vocab,
                                            )
                                            && parsed.zone.is_none()
                                            && parsed.extra_zones.is_empty()
                                            && parsed.controller.is_none()
                                            && parsed.owner.is_none()
                                            && parsed.named.is_none()
                                            && parsed.predicates.is_empty()
                                            && parsed.flags.is_empty()
                                            && let Ok(sf) = lower_spell_filter(&parsed, vocabulary)
                                        {
                                            events.push(TriggerEvent::SpellCast {
                                                caster: scope,
                                                filter: sf,
                                                self_cast: false,
                                                include_copy: false,
                                                min_mana_value: None,
                                            });
                                            extra_cond = Some(turn_cond);
                                            handled_single = true;
                                        }
                                    }
                                }
                            }
                        }
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
            TriggerEventAst::Attacks {
                subject,
                actor,
                qualifier: Some(q),
                grouped,
            } if !has_intervening_if
                && q.trim()
                    .to_ascii_lowercase()
                    .ends_with(" for the first time each turn") =>
            {
                let q_low = q.trim().to_ascii_lowercase();
                if let Some(stripped) = q_low.strip_suffix(" for the first time each turn") {
                    let inner_ev = TriggerEventAst::Attacks {
                        subject: subject.clone(),
                        actor: *actor,
                        qualifier: Some(stripped.trim().to_owned()),
                        grouped: *grouped,
                    };
                    let (e, g) = lower_trigger_event(&inner_ev, vocabulary)?;
                    if g != TriggerGrouping::EachOccurrence {
                        grouping = g;
                    }
                    events.push(e);
                    qualified_first_time = true;
                    handled_single = true;
                }
            }
            TriggerEventAst::CardDrawn { player, qualifier } => {
                let q_low = qualifier.trim().to_ascii_lowercase();
                let own_turn_related = || {
                    TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(
                            ResolvingPlayer::UniqueRelatedPlayer,
                        )),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    }))
                };
                let timing_split = q_low
                    .strip_suffix(" during an opponent's turn")
                    .map(|p| (p.trim(), TriggerCondition::OpponentsTurn))
                    .or_else(|| {
                        q_low
                            .strip_suffix(" during your turn")
                            .or_else(|| q_low.strip_suffix(" during each of your turns"))
                            .map(|p| (p.trim(), TriggerCondition::ControllersTurn))
                    })
                    .or_else(|| {
                        let p = q_low
                            .strip_suffix(" during their turn")
                            .or_else(|| q_low.strip_suffix(" during that player's turn"))
                            .or_else(|| q_low.strip_suffix(" during each of their turns"))?;
                        let cond = match player {
                            PlayerRefAst::You => TriggerCondition::ControllersTurn,
                            PlayerRefAst::AnyPlayer
                            | PlayerRefAst::EachPlayer
                            | PlayerRefAst::AnyOpponent
                            | PlayerRefAst::EachOpponent => own_turn_related(),
                            _ => return None,
                        };
                        Some((p.trim(), cond))
                    });
                if let Some((prefix, turn_cond)) = timing_split {
                    let inner_ev = TriggerEventAst::CardDrawn {
                        player: *player,
                        qualifier: prefix.to_owned(),
                    };
                    let (e, g) = lower_trigger_event(&inner_ev, vocabulary)?;
                    if g != TriggerGrouping::EachOccurrence {
                        grouping = g;
                    }
                    events.push(e);
                    extra_cond = Some(turn_cond);
                    handled_single = true;
                }
            }
            _ => {}
        }
    }
    if !handled_single && unwrapped_events.len() == 2 {
        fn is_plain_cycled_other_or_any(ev: &TriggerEventAst) -> Option<&ObjectFilterAst> {
            if let TriggerEventAst::Cycled { subject } = ev
                && let SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) =
                    subject.as_ref()
                && filter_only_has(
                    f,
                    FilterAllow {
                        controller: true,
                        flags: true,
                        ..FilterAllow::NONE
                    },
                )
                && matches!(f.controller, None | Some(ControllerRelation::You))
                && (f.flags.is_empty() || f.flags == AstFilterFlags::EXCLUDE_SELF)
            {
                return Some(f.as_ref());
            }
            None
        }
        let matched_cycle_or_discard = match (&unwrapped_events[0], &unwrapped_events[1]) {
            (
                cycled_ev,
                disc_ev @ TriggerEventAst::CardDiscarded {
                    player: PlayerRefAst::You,
                    filter,
                },
            ) if is_plain_cycled_other_or_any(cycled_ev) == Some(filter.as_ref()) => Some(disc_ev),
            (
                disc_ev @ TriggerEventAst::CardDiscarded {
                    player: PlayerRefAst::You,
                    filter,
                },
                cycled_ev,
            ) if is_plain_cycled_other_or_any(cycled_ev) == Some(filter.as_ref()) => Some(disc_ev),
            _ => None,
        };
        if let Some(disc_ev) = matched_cycle_or_discard {
            // Engine Discarded / DiscardedMatching triggers on both regular discards and
            // cycling discards (CR 702.29a); emitting both Cycled and CardDiscarded would
            // double-trigger when cycling a card.
            let (e, g) = lower_trigger_event(disc_ev, vocabulary)?;
            if g != TriggerGrouping::EachOccurrence {
                if qualified_first_time && g == TriggerGrouping::OneOrMoreByPlayer {
                    return Err("oracle:unsupported-trigger-qualifier");
                }
                grouping = g;
            }
            events.push(e);
            handled_single = true;
        }
    }
    if !handled_single {
        for ev in &unwrapped_events {
            let (e, g) = lower_trigger_event(ev, vocabulary)?;
            if g != TriggerGrouping::EachOccurrence {
                if qualified_first_time && g == TriggerGrouping::OneOrMoreByPlayer {
                    return Err("oracle:unsupported-trigger-qualifier");
                }
                grouping = g;
            }
            events.push(e);
        }
    }
    if qualified_first_time {
        if grouping == TriggerGrouping::OneOrMoreByPlayer {
            return Err("oracle:unsupported-trigger-qualifier");
        }
        grouping = TriggerGrouping::OneOrMoreOnceEachTurn;
    }
    if let Some(qc) = qualified_cond {
        extra_cond = Some(match extra_cond {
            Some(prev) => TriggerCondition::And(Box::new(prev), Box::new(qc)),
            None => qc,
        });
    }
    let mut event = wrap_trigger_events_any_of(events)?;
    if let Some(expected_tapped) = qualified_enters_tapped {
        apply_enters_tapped_filter(&mut event, expected_tapped)?;
    }
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
    if !consumed_if
        && let (
            TriggerEvent::Attacks {
                subject,
                mode: mode @ AttackTriggerMode::Individual { .. },
            },
            Some(ConditionAst::PlayerActionThisTurn { action, .. }),
        ) = (&mut event, &trig.intervening_if)
        && *subject == ObjectFilter::creature().opposing()
        && action
            .trim()
            .eq_ignore_ascii_case("they attacked you and/or a planeswalker you control")
    {
        *mode = AttackTriggerMode::AtControllerOrPlaneswalker;
        grouping = TriggerGrouping::OneOrMoreTogether;
        consumed_if = true;
    }
    for ev in &unwrapped_events {
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
    if qualified_while_exiled {
        spec = spec.in_zone(spellwake_engine::program::TriggerZone::Any);
        let exile_cond = TriggerCondition::Eval(Box::new(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::InZone(
                spellwake_engine::program::SourceZoneKind::Exile,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }));
        extra_cond = Some(match extra_cond {
            Some(prev) => TriggerCondition::And(Box::new(prev), Box::new(exile_cond)),
            None => exile_cond,
        });
    }
    let mut guard = None;
    if let Some(ec) = extra_cond {
        spec = spec.only_if(ec);
    }
    if !consumed_if && let Some(cond) = &trig.intervening_if {
        let is_self_subj = |s: &SubjectRefAst| match s {
            SubjectRefAst::SelfObject => true,
            SubjectRefAst::Anaphora(a) => {
                is_self_anaphora_label(a) || a.trim().eq_ignore_ascii_case("it")
            }
            _ => false,
        };
        let is_self_in_gy = |c: &ConditionAst| {
            matches!(
                c,
                ConditionAst::Marker(GameMarker::InZone {
                    subject,
                    zone: ZoneFilter::YourGraveyard,
                }) | ConditionAst::SubjectInZone {
                    subject,
                    zone: ZoneFilter::YourGraveyard,
                } if is_self_subj(subject)
            )
        };
        let is_self_in_gy_or_bf = |c: &ConditionAst| {
            matches!(
                c,
                ConditionAst::PlayerActionThisTurn {
                    player: PlayerRefAst::You,
                    action,
                } if matches!(
                    action.trim().trim_end_matches('.').to_ascii_lowercase().as_str(),
                    "~ is in your graveyard or on the battlefield"
                        | "~ is on the battlefield or in your graveyard"
                )
            )
        };
        let is_self_in_exile = |c: &ConditionAst| match c {
            ConditionAst::Marker(GameMarker::InZone {
                subject,
                zone: ZoneFilter::Exile,
            })
            | ConditionAst::SubjectInZone {
                subject,
                zone: ZoneFilter::Exile,
            } => is_self_subj(subject),
            ConditionAst::HasStatus { subject, status } => {
                is_self_subj(subject) && status.trim().eq_ignore_ascii_case("exiled")
            }
            _ => false,
        };
        let exile_zone_cond = || {
            TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(
                    spellwake_engine::program::SourceZoneKind::Exile,
                )),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            }))
        };
        let apply_exile_zone = |spec: &mut TriggerSpec| {
            spec.zone = spellwake_engine::program::TriggerZone::Any;
            let ec = exile_zone_cond();
            let combined = match spec.intervening_if.take() {
                Some(prev) => TriggerCondition::And(Box::new(prev), Box::new(ec)),
                None => ec,
            };
            spec.intervening_if = Some(combined);
        };
        let synthetic_cond: Option<ConditionAst>;
        let remaining = if matches!(
            cond,
            ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::Marker(GameMarker::DiedThisWay))
        ) && let TriggerEvent::LeavesBattlefield { subject } = &spec.event
        {
            let subj = *subject;
            spec.event = TriggerEvent::LeavesBattlefieldWithoutDying { subject: subj };
            None
        } else if let ConditionAst::MatchesFilter { subject, filter } = cond
            && is_self_subj(subject)
            && filter.zone == Some(ZoneFilter::Exile)
            && filter.extra_zones.is_empty()
            && filter.any_types.is_empty()
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
            && filter.tapped.is_none()
            && filter.token.is_none()
            && filter.combat.is_none()
            && filter.controller.is_none()
            && filter.owner.is_none()
            && filter.named.is_none()
            && filter.predicates.is_empty()
            && !filter.any_spell
            && (filter.flags.is_empty() || filter.flags == AstFilterFlags::WITH_ANY_COUNTER)
        {
            apply_exile_zone(&mut spec);
            if let Some(ck) = &filter.counter_kind {
                synthetic_cond = Some(ConditionAst::CounterCompare {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: ck.clone(),
                    op: CompareOp::Ge,
                    value: 1,
                });
                synthetic_cond.as_ref()
            } else if filter.flags == AstFilterFlags::WITH_ANY_COUNTER {
                synthetic_cond = Some(ConditionAst::CounterCompare {
                    subject: Box::new(SubjectRefAst::SelfObject),
                    counter_kind: "any".to_string(),
                    op: CompareOp::Ge,
                    value: 1,
                });
                synthetic_cond.as_ref()
            } else {
                None
            }
        } else if is_self_in_gy(cond) {
            spec = spec.in_zone(spellwake_engine::program::TriggerZone::Graveyard);
            None
        } else if is_self_in_gy_or_bf(cond) {
            spec = spec.in_zone(spellwake_engine::program::TriggerZone::Any);
            Some(cond)
        } else if is_self_in_exile(cond) {
            apply_exile_zone(&mut spec);
            None
        } else if let ConditionAst::And(a, b) = cond {
            if is_self_in_gy(a) {
                spec = spec.in_zone(spellwake_engine::program::TriggerZone::Graveyard);
                Some(b.as_ref())
            } else if is_self_in_gy(b) {
                spec = spec.in_zone(spellwake_engine::program::TriggerZone::Graveyard);
                Some(a.as_ref())
            } else if is_self_in_gy_or_bf(a) || is_self_in_gy_or_bf(b) {
                spec = spec.in_zone(spellwake_engine::program::TriggerZone::Any);
                Some(cond)
            } else if is_self_in_exile(a) {
                apply_exile_zone(&mut spec);
                Some(b.as_ref())
            } else if is_self_in_exile(b) {
                apply_exile_zone(&mut spec);
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

fn normalize_trigger_from_zone(raw: &str) -> String {
    let low = raw.trim().to_ascii_lowercase();
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
        "anywhere other than the battlefield"
        | "anywhere other than battlefield"
        | "not_battlefield" => "not_battlefield".to_owned(),
        _ => low,
    }
}

fn zone_filter_to_zone_name(zone: ZoneFilter) -> Option<&'static str> {
    match zone {
        ZoneFilter::Battlefield => Some("battlefield"),
        ZoneFilter::Stack => Some("stack"),
        ZoneFilter::Exile => Some("exile"),
        ZoneFilter::YourGraveyard => Some("your_graveyard"),
        ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards => {
            Some("opponent_graveyard")
        }
        ZoneFilter::AnyGraveyard
        | ZoneFilter::AllGraveyards
        | ZoneFilter::SingleGraveyard
        | ZoneFilter::ItsOwnerGraveyard => Some("graveyard"),
        ZoneFilter::YourHand => Some("your_hand"),
        ZoneFilter::OpponentHand | ZoneFilter::OpponentsHands => Some("opponent_hand"),
        ZoneFilter::AnyHand | ZoneFilter::EachPlayerHand => Some("hand"),
        ZoneFilter::YourLibrary
        | ZoneFilter::OpponentLibrary
        | ZoneFilter::EachPlayerLibrary
        | ZoneFilter::AnyLibrary => Some("library"),
        _ => None,
    }
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
            let mut parts = Vec::new();
            expand_trigger_subject_parts(subject, &mut parts);
            if parts.len() >= 2 {
                let mut sub_events = Vec::with_capacity(parts.len());
                let mut common_grouping: Option<TriggerGrouping> = None;
                for part in parts {
                    let sub_ev = TriggerEventAst::ZoneChange {
                        from: from.clone(),
                        to: to.clone(),
                        subject: Box::new(part.clone()),
                        grouped: *grouped,
                        entering_controller: *entering_controller,
                    };
                    let (e, g) = lower_trigger_event(&sub_ev, vocabulary)?;
                    if let Some(prev) = common_grouping {
                        if prev != g {
                            return Err("oracle:unsupported-zone-trigger");
                        }
                    } else {
                        common_grouping = Some(g);
                    }
                    sub_events.push(e);
                }
                return Ok((
                    wrap_trigger_events_any_of(sub_events)?,
                    common_grouping.unwrap_or(TriggerGrouping::EachOccurrence),
                ));
            }
            let eff_subject = parts.into_iter().next().unwrap_or(subject.as_ref());
            if let SubjectRefAst::InZone {
                subject: inner,
                zone,
            } = eff_subject
            {
                let z_name =
                    zone_filter_to_zone_name(*zone).ok_or("oracle:unsupported-zone-trigger")?;
                let next_from = match from.as_deref().map(normalize_trigger_from_zone) {
                    None => Some(z_name.to_owned()),
                    Some(prev) if prev == "any" => Some(z_name.to_owned()),
                    Some(prev) if prev == z_name => from.clone(),
                    Some(_) => return Err("oracle:unsupported-zone-trigger"),
                };
                let sub_ev = TriggerEventAst::ZoneChange {
                    from: next_from,
                    to: to.clone(),
                    subject: inner.clone(),
                    grouped: *grouped,
                    entering_controller: *entering_controller,
                };
                return lower_trigger_event(&sub_ev, vocabulary);
            }
            let grouping = if *grouped {
                TriggerGrouping::OneOrMoreTogether
            } else {
                TriggerGrouping::EachOccurrence
            };
            let mut filter = lower_trigger_subject_filter(eff_subject, vocabulary)?;
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
            let from_norm = from.as_deref().map(normalize_trigger_from_zone);
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
                    eff_subject
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
                (Some("not_battlefield"), Some("battlefield" | "battlefield_transformed")) => Ok((
                    TriggerEvent::Enters {
                        subject: filter,
                        from_zone: TriggerFromZone::NotBattlefield,
                        cast_origin: None,
                        attached_to_creature: false,
                    },
                    grouping,
                )),
                (_, _)
                    if {
                        let (a, b) = filter.clauses();
                        let non_pt = |c: ObjectClause| {
                            c.required_counter_kind.is_some_and(|k| {
                                !matches!(k, CounterKind::PlusOne | CounterKind::MinusOne)
                            }) || c.forbidden_counter_kind.is_some_and(|k| {
                                !matches!(k, CounterKind::PlusOne | CounterKind::MinusOne)
                            })
                        };
                        non_pt(a) || b.is_some_and(non_pt)
                    } =>
                {
                    Err("oracle:unsupported-object-filter")
                }
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
                        Some("not_battlefield") => TriggerFromZone::NotBattlefield,
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
                    let mode = if matches!(eff_subject, SubjectRefAst::SelfObject) {
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
            let mut parts = Vec::new();
            expand_trigger_subject_parts(subject, &mut parts);
            let mut sub_events = Vec::with_capacity(parts.len());
            for part in parts {
                let mut cur = part;
                let mut from_zone = TriggerFromZone::Any;
                while let SubjectRefAst::InZone {
                    subject: inner,
                    zone,
                } = cur
                {
                    let mapped = match zone {
                        ZoneFilter::Battlefield => TriggerFromZone::Battlefield,
                        ZoneFilter::YourGraveyard => TriggerFromZone::ControllerGraveyard,
                        ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards => {
                            TriggerFromZone::OpponentGraveyard
                        }
                        ZoneFilter::AnyGraveyard
                        | ZoneFilter::AllGraveyards
                        | ZoneFilter::SingleGraveyard
                        | ZoneFilter::ItsOwnerGraveyard => TriggerFromZone::Graveyard,
                        ZoneFilter::YourHand
                        | ZoneFilter::OpponentHand
                        | ZoneFilter::OpponentsHands
                        | ZoneFilter::AnyHand
                        | ZoneFilter::EachPlayerHand => TriggerFromZone::Hand,
                        ZoneFilter::Exile => TriggerFromZone::Exile,
                        ZoneFilter::YourLibrary
                        | ZoneFilter::OpponentLibrary
                        | ZoneFilter::EachPlayerLibrary
                        | ZoneFilter::AnyLibrary => TriggerFromZone::Library,
                        _ => return Err("oracle:unsupported-trigger-subject"),
                    };
                    if from_zone != TriggerFromZone::Any && from_zone != mapped {
                        return Err("oracle:unsupported-trigger-subject");
                    }
                    from_zone = mapped;
                    cur = inner.as_ref();
                }
                let subj_filters = lower_trigger_subject_filters(cur, vocabulary)?;
                for subj in subj_filters {
                    sub_events.push(TriggerEvent::Exiled {
                        subject: subj,
                        from_zone,
                    });
                }
            }
            Ok((
                wrap_trigger_events_any_of(sub_events)?,
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
            let grouping = if matches!(
                mode,
                AttackTriggerMode::AtEnchantedPlayer | AttackTriggerMode::AtController
            ) {
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
                "each opponent's precombat main phase"
                | "each of each opponent's precombat main phases"
                | "each opponent's first main phase"
                | "an opponent's precombat main phase" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PrecombatMain,
                            player: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each opponent's postcombat main phase"
                | "each of each opponent's postcombat main phases"
                | "each opponent's second main phase"
                | "an opponent's postcombat main phase" => {
                    return Ok((
                        TriggerEvent::StepBegan {
                            step: Step::PostcombatMain,
                            player: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each opponent's main phase"
                | "each of each opponent's main phases"
                | "an opponent's main phase" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StepBegan {
                                step: Step::PrecombatMain,
                                player: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                            },
                            TriggerEvent::StepBegan {
                                step: Step::PostcombatMain,
                                player: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "each player's main phase" | "each main phase" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StepBegan {
                                step: Step::PrecombatMain,
                                player: TriggerPlayerScope::Relation(
                                    PlayerRelation::AnyParticipant,
                                ),
                            },
                            TriggerEvent::StepBegan {
                                step: Step::PostcombatMain,
                                player: TriggerPlayerScope::Relation(
                                    PlayerRelation::AnyParticipant,
                                ),
                            },
                        ]),
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
                "combat on each opponent's turn"
                | "combat on an opponent's turn"
                | "combat on each other player's turn" => {
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
                | "equipped permanent's controller"
                | "equipped permanent's controller's"
                | "the controller of enchanted creature"
                | "the controller of enchanted permanent"
                | "the controller of enchanted land"
                | "the controller of enchanted artifact"
                | "the controller of enchanted enchantment"
                | "the controller of enchanted planeswalker"
                | "the controller of equipped creature"
                | "the controller of equipped permanent"
                | "controller of enchanted creature"
                | "controller of enchanted permanent"
                | "controller of enchanted land"
                | "controller of enchanted artifact"
                | "controller of enchanted enchantment"
                | "controller of enchanted planeswalker"
                | "controller of equipped creature"
                | "controller of equipped permanent"
                | "attached_host"
                | "attached_host's"
                | "attached_host_controller" => TriggerPlayerScope::AttachedHostController,
                "monarch" | "monarch's" | "monarch's turn" | "the monarch" | "the monarch's"
                | "the monarch's turn" => TriggerPlayerScope::Monarch,
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
                PlayerRefAst::EnchantedPlayer => TriggerPlayerScope::EnchantedPlayer,
                _ => return Err("oracle:unsupported-spell-caster"),
            };
            let mut pred_nth: Option<u8> = None;
            let (sf, min_mv) = match spell {
                Some(f) => {
                    let mut cloned = f.as_ref().clone();
                    cloned.predicates.retain(|p| {
                        if let spellwake_oracle_compiler::ast::FilterPredicateAst::NthSpellCastThisTurn(
                            n,
                        ) = p
                            && pred_nth.is_none()
                        {
                            pred_nth = Some(*n);
                            false
                        } else {
                            true
                        }
                    });
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
            let effective_nth = match (*nth_each_turn, pred_nth) {
                (Some(a), Some(b)) if a != b => return Err("oracle:conflicting-nth-spell"),
                (Some(a), _) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            let mut sf = sf;
            match targets {
                None => {}
                Some(t) if matches!(t.as_ref(), SubjectRefAst::SelfObject) => {
                    sf = sf.targeting_only_source();
                }
                Some(t) if !*self_spell && effective_nth.is_none() && min_mv.is_none() => {
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
                    if let Some(inst_sorc) = inst_sorc_only {
                        let mut parts = Vec::new();
                        expand_trigger_subject_parts(t, &mut parts);
                        if !parts.is_empty() {
                            let mut sub_events = Vec::with_capacity(parts.len());
                            let mut all_ok = true;
                            for part in parts {
                                if let SubjectRefAst::Player(p) = part {
                                    let Ok(rel) = lower_player_relation(*p) else {
                                        all_ok = false;
                                        break;
                                    };
                                    match caster {
                                        PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                                            sub_events.push(TriggerEvent::PlayerTargeted {
                                                player: rel,
                                                by_opponent: true,
                                                spell_only: true,
                                                instant_or_sorcery_only: inst_sorc,
                                            });
                                        }
                                        PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                                            sub_events.push(TriggerEvent::PlayerTargeted {
                                                player: rel,
                                                by_opponent: false,
                                                spell_only: true,
                                                instant_or_sorcery_only: inst_sorc,
                                            });
                                        }
                                        _ => {
                                            all_ok = false;
                                            break;
                                        }
                                    }
                                } else if let Ok(tgt_filters) =
                                    lower_trigger_subject_filters(part, vocabulary)
                                {
                                    for tgt_filter in tgt_filters {
                                        let event = match caster {
                                            PlayerRefAst::You => {
                                                TriggerEvent::TargetedByController {
                                                    subject: tgt_filter,
                                                    spell_only: true,
                                                    instant_or_sorcery_only: inst_sorc,
                                                }
                                            }
                                            PlayerRefAst::AnyOpponent
                                            | PlayerRefAst::EachOpponent => {
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
                                            _ => {
                                                all_ok = false;
                                                break;
                                            }
                                        };
                                        sub_events.push(event);
                                    }
                                    if !all_ok {
                                        break;
                                    }
                                } else {
                                    all_ok = false;
                                    break;
                                }
                            }
                            if all_ok && !sub_events.is_empty() {
                                return Ok((
                                    wrap_trigger_events_any_of(sub_events)?,
                                    TriggerGrouping::OneOrMoreTogether,
                                ));
                            }
                        }
                    }
                    return Err("oracle:unsupported-spell-target");
                }
                Some(_) => return Err("oracle:unsupported-spell-target"),
            }
            if let Some(nth) = effective_nth {
                let nz = NonZeroU32::new(u32::from(nth)).ok_or("oracle:zero-nth")?;
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
            let mut player_level_attack = false;
            let mut subjects = match (subject.as_deref(), actor) {
                (Some(SubjectRefAst::Player(PlayerRefAst::You)), None) => {
                    player_level_attack = true;
                    vec![ObjectFilter::creature().controlled()]
                }
                (
                    Some(SubjectRefAst::Player(
                        PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
                    )),
                    None,
                ) => {
                    player_level_attack = true;
                    vec![ObjectFilter::creature().opposing()]
                }
                (
                    Some(SubjectRefAst::Player(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer)),
                    None,
                ) => {
                    player_level_attack = true;
                    vec![ObjectFilter::creature()]
                }
                (Some(s), _) => lower_trigger_subject_filters(s, vocabulary)?,
                (None, None | Some(PlayerRefAst::You)) => {
                    vec![ObjectFilter::creature().controlled()]
                }
                (None, Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent)) => {
                    vec![ObjectFilter::creature().opposing()]
                }
                (None, Some(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer)) => {
                    vec![ObjectFilter::creature()]
                }
                _ => return Err("oracle:unsupported-trigger-event"),
            };
            let mut grouping = if *grouped || (player_level_attack && qualifier.is_none()) {
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
                Some("one or more of your opponents") => {
                    grouping = TriggerGrouping::OneOrMoreTogether;
                    AttackTriggerMode::AtOpponent
                }
                Some("player" | "a player" | "one or more players") => AttackTriggerMode::AtPlayer,
                Some("planeswalker" | "a planeswalker") => AttackTriggerMode::AtPlaneswalker,
                Some("enchanted_player" | "enchanted player") => {
                    AttackTriggerMode::AtEnchantedPlayer
                }
                Some("the monarch" | "monarch") => AttackTriggerMode::AtMonarch,
                Some("the player who has the initiative" | "initiative" | "has the initiative") => {
                    AttackTriggerMode::AtInitiative
                }
                Some("first_time_each_turn") => AttackTriggerMode::FirstThisTurn {
                    highest_life: false,
                },
                Some("two_or_more") => AttackTriggerMode::GroupAtLeast(2),
                Some("one or more equipped creatures") => AttackTriggerMode::EquippedAtPlayer,
                Some(
                    "the most life"
                    | "the most life or tied for most life"
                    | "the most life or tied for the most life",
                ) => AttackTriggerMode::HighestLifeDefender,
                Some(other) => {
                    let core = if subject.is_none() && actor.is_some() {
                        other.strip_prefix("with ").map_or(other, str::trim)
                    } else {
                        other
                    };
                    if subject.is_none() && actor.is_some() && core == "one or more creatures" {
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::Individual {
                            capture_defender: false,
                        }
                    } else if subject.is_none()
                        && matches!(actor, None | Some(PlayerRefAst::You))
                        && core == "your commander"
                    {
                        subjects = vec![ObjectFilter::Clause(ObjectClause {
                            owner: ControllerFilter::You,
                            flags: ObjectFilterFlags::COMMANDER_ONLY,
                            ..ObjectClause::any()
                        })];
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::Individual {
                            capture_defender: false,
                        }
                    } else if subject.is_none()
                        && matches!(actor, None | Some(PlayerRefAst::You))
                        && matches!(
                            core,
                            "a commander you control" | "your commander you control"
                        )
                    {
                        subjects = vec![ObjectFilter::Clause(ObjectClause {
                            controller: ControllerFilter::You,
                            flags: ObjectFilterFlags::COMMANDER_ONLY,
                            ..ObjectClause::any()
                        })];
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::Individual {
                            capture_defender: false,
                        }
                    } else if subject.is_none()
                        && matches!(actor, None | Some(PlayerRefAst::You))
                        && matches!(
                            core,
                            "~ and/or your commander" | "this creature and/or your commander"
                        )
                    {
                        subjects = vec![
                            ObjectFilter::self_object(),
                            ObjectFilter::Clause(ObjectClause {
                                owner: ControllerFilter::You,
                                flags: ObjectFilterFlags::COMMANDER_ONLY,
                                ..ObjectClause::any()
                            }),
                        ];
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::Individual {
                            capture_defender: false,
                        }
                    } else if other
                        == "you with two or more creatures, draws their second card each turn, or casts their second spell each turn"
                        && (matches!(
                            actor,
                            Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent)
                        ) || (actor.is_none()
                            && matches!(
                                subject.as_deref(),
                                Some(SubjectRefAst::Player(
                                    PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent
                                ))
                            )))
                    {
                        let second = NonZeroU16::new(2).ok_or("oracle:zero-nth")?;
                        let second_spell = NonZeroU32::new(2).ok_or("oracle:zero-nth")?;
                        return Ok((
                            TriggerEvent::AnyOf(vec![
                                TriggerEvent::Attacks {
                                    subject: ObjectFilter::creature().opposing(),
                                    mode: AttackTriggerMode::OpponentsAtController { minimum: 2 },
                                },
                                TriggerEvent::PlayerAction {
                                    player: PlayerRelation::Opponent,
                                    action: PlayerTriggerAction::DrewCard {
                                        nth_each_turn: Some(second),
                                    },
                                },
                                TriggerEvent::SpellCast {
                                    caster: TriggerPlayerScope::Relation(PlayerRelation::Opponent),
                                    filter: SpellFilter::any().ordinal_this_turn(second_spell),
                                    self_cast: false,
                                    include_copy: false,
                                    min_mana_value: None,
                                },
                            ]),
                            TriggerGrouping::EachOccurrence,
                        ));
                    } else if let Some(after_you) = other.strip_prefix("you with ")
                        && (matches!(
                            actor,
                            Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent)
                        ) || (actor.is_none()
                            && matches!(
                                subject.as_deref(),
                                Some(SubjectRefAst::Player(
                                    PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent
                                ))
                            )))
                        && let Some((n, "creatures")) = parse_attack_count_prefix(after_you.trim())
                    {
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::OpponentsAtController { minimum: n }
                    } else if let Some(n_str) = core
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
                            "the monarch" | "monarch" => AttackTriggerMode::AtMonarch,
                            "the player who has the initiative" => AttackTriggerMode::AtInitiative,
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
                            subjects = vec![f];
                        }
                        grouping = if matches!(
                            def_mode,
                            AttackTriggerMode::AtPlayer
                                | AttackTriggerMode::AtOpponent
                                | AttackTriggerMode::AtController
                                | AttackTriggerMode::AtControllerOrPlaneswalker
                                | AttackTriggerMode::AtEnchantedPlayer
                                | AttackTriggerMode::AtOpponentOrPlaneswalker
                                | AttackTriggerMode::AtMonarch
                                | AttackTriggerMode::AtInitiative
                        ) {
                            TriggerGrouping::OneOrMoreByPlayer
                        } else {
                            TriggerGrouping::OneOrMoreTogether
                        };
                        def_mode
                    } else if let Some(flt_raw) = core.strip_prefix("one or more ")
                        && subject.is_none()
                        && actor.is_some()
                    {
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                            flt_raw.trim(),
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
                        subjects = vec![f];
                        grouping = TriggerGrouping::OneOrMoreTogether;
                        AttackTriggerMode::GroupAtLeast(1)
                    } else if let Some((count_prefix, flt_raw)) = parse_attack_count_prefix(core) {
                        if flt_raw != "creatures" {
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
                            subjects = vec![f];
                        }
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
                        | AttackTriggerMode::AtController
                        | AttackTriggerMode::AtControllerOrPlaneswalker
                        | AttackTriggerMode::AtEnchantedPlayer
                        | AttackTriggerMode::AtOpponentOrPlaneswalker
                        | AttackTriggerMode::AtMonarch
                        | AttackTriggerMode::AtInitiative
                )
            {
                grouping = TriggerGrouping::OneOrMoreByPlayer;
            }
            if subjects.len() > 1
                && matches!(
                    mode,
                    AttackTriggerMode::GroupAtLeast(_)
                        | AttackTriggerMode::GroupTotalPowerAtLeast(_)
                        | AttackTriggerMode::OpponentsAtController { .. }
                )
            {
                return Err("oracle:unsupported-trigger-subject");
            }
            let sub_events = subjects
                .into_iter()
                .map(|subj| TriggerEvent::Attacks {
                    subject: subj,
                    mode,
                })
                .collect();
            Ok((wrap_trigger_events_any_of(sub_events)?, grouping))
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
        TriggerEventAst::AttacksUnblocked { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| {
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::Unblocked,
                }
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Blocks { subject, attacker } => {
            let subjects = lower_trigger_subject_filters(subject, vocabulary)?;
            let mut sub_events = Vec::new();
            match attacker {
                None => {
                    for subj in subjects {
                        sub_events.push(TriggerEvent::BlocksOrBlocked {
                            subject: subj,
                            kind: BlockTriggerKind::Blocks,
                        });
                    }
                }
                Some(atk) => {
                    let attackers = lower_trigger_subject_filters(atk, vocabulary)?;
                    for subj in &subjects {
                        for atk_filter in &attackers {
                            sub_events.push(TriggerEvent::BlocksOrBlocked {
                                subject: *subj,
                                kind: BlockTriggerKind::BlocksMatching(Box::new(*atk_filter)),
                            });
                        }
                    }
                }
            }
            Ok((
                wrap_trigger_events_any_of(sub_events)?,
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesBlocked { subject, blocker } => {
            let subjects = lower_trigger_subject_filters(subject, vocabulary)?;
            let mut sub_events = Vec::new();
            match blocker {
                None => {
                    for subj in subjects {
                        sub_events.push(TriggerEvent::BlocksOrBlocked {
                            subject: subj,
                            kind: BlockTriggerKind::BecomesBlocked {
                                capture_defender: false,
                            },
                        });
                    }
                }
                Some(blk) => {
                    let blockers = lower_trigger_subject_filters(blk, vocabulary)?;
                    for subj in &subjects {
                        for blk_filter in &blockers {
                            sub_events.push(TriggerEvent::BlocksOrBlocked {
                                subject: *subj,
                                kind: BlockTriggerKind::BlockedByMatching(Box::new(*blk_filter)),
                            });
                        }
                    }
                }
            }
            Ok((
                wrap_trigger_events_any_of(sub_events)?,
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::BecomesBlockedByCount {
            subject,
            min_blockers,
        } if *min_blockers >= 1 => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| {
                TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: BlockTriggerKind::BlockedByAtLeast {
                        min_count: *min_blockers,
                        blocker_filter: None,
                    },
                }
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::BlocksOrBecomesBlockedBy {
            subject,
            other_filter,
            blocks,
            becomes_blocked,
            min_count,
        } if *min_count >= 1 && other_filter.zone.is_none() => {
            let subjects = lower_trigger_subject_filters(subject, vocabulary)?;
            if *min_count >= 2 {
                if *blocks || !*becomes_blocked {
                    return Err("oracle:unsupported-trigger-event");
                }
                let plain_any_creature = other_filter.is_empty()
                    || (other_filter.any_types == AstCardTypeMask::CREATURE
                        && filter_only_has(
                            other_filter,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        ));
                let blocker_filter = if plain_any_creature {
                    None
                } else {
                    Some(Box::new(lower_object_filter(other_filter, vocabulary)?))
                };
                let sub_events = subjects
                    .into_iter()
                    .map(|subj| TriggerEvent::BlocksOrBlocked {
                        subject: subj,
                        kind: BlockTriggerKind::BlockedByAtLeast {
                            min_count: *min_count,
                            blocker_filter: blocker_filter.clone(),
                        },
                    })
                    .collect();
                return Ok((
                    wrap_trigger_events_any_of(sub_events)?,
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let obj = lower_object_filter(other_filter, vocabulary)?;
            let kind = match (*blocks, *becomes_blocked) {
                (true, false) => BlockTriggerKind::BlocksMatching(Box::new(obj)),
                (false, true) => BlockTriggerKind::BlockedByMatching(Box::new(obj)),
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
                        BlockTriggerKind::BlocksOrBlockedByEach { required_subtype }
                    } else {
                        BlockTriggerKind::BlocksOrBlockedByMatching(Box::new(obj))
                    }
                }
                (false, false) => return Err("oracle:unsupported-trigger-event"),
            };
            let sub_events = subjects
                .into_iter()
                .map(|subj| TriggerEvent::BlocksOrBlocked {
                    subject: subj,
                    kind: kind.clone(),
                })
                .collect();
            Ok((
                wrap_trigger_events_any_of(sub_events)?,
                TriggerGrouping::EachOccurrence,
            ))
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
                && let Some(rec_subj) = recipient.as_deref()
                && !matches!(
                    rec_subj,
                    SubjectRefAst::Player(_) | SubjectRefAst::TargetPlayerOrPlaneswalker { .. }
                )
                && let Ok(subj_filters) = lower_trigger_subject_filters(rec_subj, vocabulary)
            {
                let sub_events = subj_filters
                    .into_iter()
                    .map(|subj| TriggerEvent::DealtDamage {
                        subject: subj,
                        linked_player: false,
                    })
                    .collect();
                let grouping = if *grouped {
                    TriggerGrouping::OneOrMoreTogether
                } else {
                    TriggerGrouping::EachOccurrence
                };
                return Ok((wrap_trigger_events_any_of(sub_events)?, grouping));
            }
            let sources = match source {
                Some(s) => lower_trigger_subject_filters(s, vocabulary)?,
                None => vec![ObjectFilter::any()],
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
                Some(SubjectRefAst::Compound(items)) if (2..=3).contains(&items.len()) => {
                    let mut has_controller = false;
                    let mut has_opponent = false;
                    let mut has_any_player = false;
                    let mut has_pw = false;
                    let mut has_battle = false;
                    let mut all_recognized = true;
                    let plain_types = FilterAllow {
                        types: true,
                        ..FilterAllow::NONE
                    };
                    let types_or_ctrl = FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    };
                    for item in items {
                        match item {
                            SubjectRefAst::Player(PlayerRefAst::You) => has_controller = true,
                            SubjectRefAst::Player(
                                PlayerRefAst::AnyOpponent
                                | PlayerRefAst::EachOpponent
                                | PlayerRefAst::DefendingPlayer,
                            ) => has_opponent = true,
                            SubjectRefAst::Player(
                                PlayerRefAst::AnyPlayer
                                | PlayerRefAst::EachPlayer
                                | PlayerRefAst::TargetPlayer
                                | PlayerRefAst::ThatPlayer,
                            ) => has_any_player = true,
                            SubjectRefAst::TargetPlayerOrPlaneswalker {
                                opponent_only: false,
                            } => {
                                has_any_player = true;
                                has_pw = true;
                            }
                            SubjectRefAst::TargetPlayerOrPlaneswalker {
                                opponent_only: true,
                            } => {
                                has_opponent = true;
                                has_pw = true;
                            }
                            SubjectRefAst::Group(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::PLANESWALKER
                                    && filter_only_has(f, types_or_ctrl) =>
                            {
                                has_pw = true;
                            }
                            SubjectRefAst::Group(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::All(f)
                                if f.any_types == AstCardTypeMask::BATTLE
                                    && filter_only_has(f, plain_types) =>
                            {
                                has_battle = true;
                            }
                            SubjectRefAst::Group(f)
                            | SubjectRefAst::Each(f)
                            | SubjectRefAst::All(f)
                                if f.any_types
                                    == AstCardTypeMask::PLANESWALKER
                                        .union(AstCardTypeMask::BATTLE)
                                    && filter_only_has(f, plain_types) =>
                            {
                                has_pw = true;
                                has_battle = true;
                            }
                            _ => {
                                all_recognized = false;
                                break;
                            }
                        }
                    }
                    if !all_recognized {
                        return Err("oracle:unsupported-damage-recipient");
                    }
                    match (
                        has_controller,
                        has_opponent,
                        has_any_player,
                        has_pw,
                        has_battle,
                    ) {
                        (true, false, false, true, false) => {
                            DamageTriggerRecipient::ControllerOrPlaneswalker
                        }
                        (false, true, false, true, false) => {
                            DamageTriggerRecipient::OpponentOrPlaneswalker
                        }
                        (false, false, true, true, false) => {
                            DamageTriggerRecipient::PlayerOrPlaneswalker
                        }
                        (false, false, true, false, true) => DamageTriggerRecipient::PlayerOrBattle,
                        (false, true, false, false, true) => {
                            DamageTriggerRecipient::OpponentOrBattle
                        }
                        (false, false, false, true, true) => {
                            DamageTriggerRecipient::PlaneswalkerOrBattle
                        }
                        (false, false, true, true, true) => {
                            DamageTriggerRecipient::PlayerPlaneswalkerOrBattle
                        }
                        _ => return Err("oracle:unsupported-damage-recipient"),
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
                    if f.any_types
                        == AstCardTypeMask::PLANESWALKER.union(AstCardTypeMask::BATTLE)
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        ) =>
                {
                    DamageTriggerRecipient::PlaneswalkerOrBattle
                }
                Some(SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f))
                    if f.any_types == AstCardTypeMask::PLANESWALKER
                        && filter_only_has(
                            f,
                            FilterAllow {
                                types: true,
                                ..FilterAllow::NONE
                            },
                        ) =>
                {
                    DamageTriggerRecipient::Planeswalker
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
            let sub_events: Vec<TriggerEvent> = if *noncombat_only {
                sources
                    .into_iter()
                    .map(|src| TriggerEvent::DealsNoncombatDamage {
                        source: src,
                        recipient: rec,
                    })
                    .collect()
            } else {
                sources
                    .into_iter()
                    .map(|src| TriggerEvent::DealsDamage {
                        source: src,
                        recipient: rec,
                        combat_only: *combat_only,
                    })
                    .collect()
            };
            Ok((wrap_trigger_events_any_of(sub_events)?, grouping))
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
            let q_low = qualifier.trim().to_ascii_lowercase();
            let is_except_first_in_draw_step = match rel {
                PlayerRelation::Controller => matches!(
                    q_low.as_str(),
                    "a card except the first one you draw in each of your draw steps"
                        | "a card except the first one you draw in your draw step"
                        | "a card other than the first one you draw in each of your draw steps"
                        | "a card other than the first one you draw in your draw step"
                ),
                PlayerRelation::Opponent | PlayerRelation::AnyParticipant => matches!(
                    q_low.as_str(),
                    "a card except the first one they draw in each of their draw steps"
                        | "a card except the first one that player draws in each of their draw steps"
                        | "a card except the first one they draw in their draw step"
                        | "a card except the first one that player draws in their draw step"
                        | "a card other than the first one they draw in each of their draw steps"
                        | "a card other than the first one that player draws in each of their draw steps"
                        | "a card other than the first one they draw in their draw step"
                        | "a card other than the first one that player draws in their draw step"
                ),
            };
            if is_except_first_in_draw_step {
                return Ok((
                    TriggerEvent::PlayerAction {
                        player: rel,
                        action: PlayerTriggerAction::DrewCardExceptFirstInDrawStep,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let is_first_or_second = q_low == "first or second"
                || (rel == PlayerRelation::Controller
                    && matches!(
                        q_low.as_str(),
                        "your first or second card each turn"
                            | "your first or second card in a turn"
                            | "your first or second card"
                    ))
                || (matches!(
                    rel,
                    PlayerRelation::Opponent | PlayerRelation::AnyParticipant
                ) && matches!(
                    q_low.as_str(),
                    "their first or second card each turn"
                        | "their first or second card in a turn"
                        | "their first or second card"
                ));
            if is_first_or_second {
                return Ok((
                    TriggerEvent::AnyOf(vec![
                        TriggerEvent::PlayerAction {
                            player: rel,
                            action: PlayerTriggerAction::DrewCard {
                                nth_each_turn: NonZeroU16::new(1),
                            },
                        },
                        TriggerEvent::PlayerAction {
                            player: rel,
                            action: PlayerTriggerAction::DrewCard {
                                nth_each_turn: NonZeroU16::new(2),
                            },
                        },
                    ]),
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let nth = match q_low.as_str() {
                "" | "any" | "a card" | "cards" | "one or more cards" => None,
                "first"
                | "1"
                | "your first card"
                | "your first card each turn"
                | "your first card in a turn"
                | "their first card"
                | "their first card each turn"
                | "their first card in a turn"
                | "a first card"
                | "the first card they draw"
                | "the first card they draw each turn" => NonZeroU16::new(1),
                "second"
                | "2"
                | "your second card"
                | "your second card each turn"
                | "your second card in a turn"
                | "their second card"
                | "their second card each turn"
                | "their second card in a turn"
                | "a second card"
                | "a second card each turn"
                | "a second card in a turn" => NonZeroU16::new(2),
                "third"
                | "3"
                | "your third card"
                | "your third card each turn"
                | "your third card in a turn"
                | "their third card"
                | "their third card each turn"
                | "their third card in a turn"
                | "a third card"
                | "a third card each turn"
                | "a third card in a turn" => NonZeroU16::new(3),
                "fourth"
                | "4"
                | "your fourth card"
                | "your fourth card each turn"
                | "your fourth card in a turn"
                | "their fourth card"
                | "their fourth card each turn"
                | "their fourth card in a turn"
                | "a fourth card"
                | "a fourth card each turn"
                | "a fourth card in a turn" => NonZeroU16::new(4),
                "fifth"
                | "5"
                | "your fifth card"
                | "your fifth card each turn"
                | "your fifth card in a turn"
                | "their fifth card"
                | "their fifth card each turn"
                | "their fifth card in a turn"
                | "a fifth card"
                | "a fifth card each turn"
                | "a fifth card in a turn" => NonZeroU16::new(5),
                "sixth"
                | "6"
                | "your sixth card"
                | "your sixth card each turn"
                | "your sixth card in a turn"
                | "their sixth card"
                | "their sixth card each turn"
                | "their sixth card in a turn"
                | "a sixth card"
                | "a sixth card each turn"
                | "a sixth card in a turn" => NonZeroU16::new(6),
                "seventh"
                | "7"
                | "your seventh card"
                | "your seventh card each turn"
                | "your seventh card in a turn"
                | "their seventh card"
                | "their seventh card each turn"
                | "their seventh card in a turn"
                | "a seventh card"
                | "a seventh card each turn"
                | "a seventh card in a turn" => NonZeroU16::new(7),
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
            Ok((
                lower_subject_trigger_event(subject, vocabulary, |subj| {
                    TriggerEvent::PlayerAction {
                        player: rel,
                        action: PlayerTriggerAction::Sacrificed(subj),
                    }
                })?,
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
                            flags: true,
                            ..FilterAllow::NONE
                        },
                    ) && matches!(f.controller, None | Some(ControllerRelation::You))
                        && (f.flags.is_empty() || f.flags == AstFilterFlags::EXCLUDE_SELF) =>
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
            let kind = if *tapped {
                StatusTriggerKind::BecomesTapped
            } else {
                StatusTriggerKind::BecomesUntapped
            };
            Ok((
                lower_subject_trigger_event(subject, vocabulary, |subj| {
                    TriggerEvent::StatusChanged {
                        subject: subj,
                        kind,
                    }
                })?,
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
        TriggerEventAst::TurnedFaceUp { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::TurnsFaceUp,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Transforms { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Transforms,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::PhaseChanged {
            subject,
            phased_out,
        } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Phased { out: *phased_out },
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Mutates { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Mutated,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
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
        TriggerEventAst::BecomesMonstrous { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::BecomesMonstrous,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::BecomesRenowned { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::BecomesRenowned,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::PermanentRegenerates { subject, this_way } => {
            if *this_way {
                return Err("oracle:unsupported-trigger-event");
            }
            Ok((
                lower_subject_trigger_event(subject, vocabulary, |subj| {
                    TriggerEvent::StatusChanged {
                        subject: subj,
                        kind: StatusTriggerKind::Regenerated,
                    }
                })?,
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::PermanentFights { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Fought,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::KeywordActionPerformed { player, action } => {
            let rel = lower_player_relation(*player)?;
            let low = action.trim().to_ascii_lowercase();
            if let Some(dmg_trig) = try_lower_damage_threshold_trigger(&low) {
                return Ok(dmg_trig);
            }
            match low.as_str() {
                "equipped creature attacks the monarch"
                | "enchanted creature attacks the monarch" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::any().attached_to_self(),
                            mode: AttackTriggerMode::AtMonarch,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you attack the monarch" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature().controlled(),
                            mode: AttackTriggerMode::AtMonarch,
                        },
                        TriggerGrouping::OneOrMoreByPlayer,
                    ));
                }
                "a creature attacks the monarch" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature(),
                            mode: AttackTriggerMode::AtMonarch,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you attack the player who has the initiative" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::creature().controlled(),
                            mode: AttackTriggerMode::AtInitiative,
                        },
                        TriggerGrouping::OneOrMoreByPlayer,
                    ));
                }
                "equipped creature attacks the player who has the initiative"
                | "enchanted creature attacks the player who has the initiative" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::any().attached_to_self(),
                            mode: AttackTriggerMode::AtInitiative,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more lands enter under an opponent's control without being played" => {
                    return Ok((
                        TriggerEvent::EntersWithoutBeingPlayed {
                            subject: ObjectFilter::land().opposing(),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a land enters under an opponent's control without being played" => {
                    return Ok((
                        TriggerEvent::EntersWithoutBeingPlayed {
                            subject: ObjectFilter::land().opposing(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more lands enter under your control without being played" => {
                    return Ok((
                        TriggerEvent::EntersWithoutBeingPlayed {
                            subject: ObjectFilter::land().controlled(),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a land enters under your control without being played" => {
                    return Ok((
                        TriggerEvent::EntersWithoutBeingPlayed {
                            subject: ObjectFilter::land().controlled(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more lands enter without being played"
                | "one or more lands enter the battlefield without being played" => {
                    return Ok((
                        TriggerEvent::EntersWithoutBeingPlayed {
                            subject: ObjectFilter::land(),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a land enters without being played"
                | "a land enters the battlefield without being played" => {
                    return Ok((
                        TriggerEvent::EntersWithoutBeingPlayed {
                            subject: ObjectFilter::land(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ saddles a mount or crews a vehicle" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::PlayerAction {
                                player: PlayerRelation::Controller,
                                action: PlayerTriggerAction::Saddled,
                            },
                            TriggerEvent::StatusChanged {
                                subject: ObjectFilter::self_object(),
                                kind: StatusTriggerKind::Crewed,
                            },
                        ]),
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ crews a vehicle" | "crews a vehicle" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::self_object(),
                            kind: StatusTriggerKind::Crewed,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you crew a vehicle" | "you crew" => {
                    return Ok((
                        TriggerEvent::StatusChanged {
                            subject: ObjectFilter::permanent().controlled(),
                            kind: StatusTriggerKind::Crewed,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
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
                "you earthbend" | "earthbend" => {
                    return Ok((
                        TriggerEvent::PlayerAction {
                            player: rel,
                            action: PlayerTriggerAction::Earthbent,
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
                "a creature you control deals combat damage to an opponent" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature().controlled(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::Opponent,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more creatures you control deal combat damage to an opponent" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature().controlled(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::Opponent,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a creature deals combat damage to you" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::Controller,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more creatures deal combat damage to you" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::Controller,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a creature deals combat damage to a player" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::AnyPlayer,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more creatures deal combat damage to a player" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::creature(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::AnyPlayer,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "combat damage is dealt to an opponent" => {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::any(),
                            combat_only: true,
                            recipient: DamageTriggerRecipient::Opponent,
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "a player other than ~'s owner controls it" => {
                    return Ok((
                        TriggerEvent::StateThreshold(StateThresholdKind::Eval(Box::new(
                            ResolutionCondition {
                                left: ValueExpr::ObjectMatches {
                                    subject: MatchObjectRef::AbilitySource,
                                    filter: Box::new(ObjectFilter::Clause(ObjectClause {
                                        owner: ControllerFilter::OwnerOpponent,
                                        ..ObjectClause::any()
                                    })),
                                },
                                comparison: Comparison::GreaterOrEqual,
                                right: ValueExpr::literal(1),
                            },
                        ))),
                        TriggerGrouping::EachOccurrence,
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
                "you activate an ability" | "you activate an ability that isn't a mana ability" => {
                    return Ok((
                        TriggerEvent::AbilityActivated {
                            player: PlayerRelation::Controller,
                            source_filter: ObjectFilter::any(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "an opponent activates an ability"
                | "an opponent activates an ability that isn't a mana ability" => {
                    return Ok((
                        TriggerEvent::AbilityActivated {
                            player: PlayerRelation::Opponent,
                            source_filter: ObjectFilter::any(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a player activates an ability"
                | "a player activates an ability that isn't a mana ability" => {
                    return Ok((
                        TriggerEvent::AbilityActivated {
                            player: PlayerRelation::AnyParticipant,
                            source_filter: ObjectFilter::any(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "an artifact becomes tapped or a player activates an artifact's ability" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StatusChanged {
                                subject: ObjectFilter::artifact(),
                                kind: StatusTriggerKind::BecomesTapped,
                            },
                            TriggerEvent::AbilityActivated {
                                player: PlayerRelation::AnyParticipant,
                                source_filter: ObjectFilter::artifact(),
                            },
                        ]),
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
                "you and/or at least one permanent you control becomes the target of a spell or ability an opponent controls" =>
                {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::PlayerTargeted {
                                player: PlayerRelation::Controller,
                                by_opponent: true,
                                spell_only: false,
                                instant_or_sorcery_only: false,
                            },
                            TriggerEvent::Targeted {
                                subject: ObjectFilter::permanent().controlled(),
                                by_opponent: true,
                                spell_only: false,
                                instant_or_sorcery_only: false,
                            },
                        ]),
                        TriggerGrouping::OneOrMoreByPlayer,
                    ));
                }
                "an opponent plays a nonbasic land" => {
                    return Ok((
                        TriggerEvent::PlayerAction {
                            player: PlayerRelation::Opponent,
                            action: PlayerTriggerAction::LandPlayedFiltered(ObjectFilter::Clause(
                                ObjectClause {
                                    forbidden_supertypes: SupertypeMask::BASIC,
                                    ..ObjectClause::land()
                                },
                            )),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "a creature you own but don't control attacks" => {
                    return Ok((
                        TriggerEvent::Attacks {
                            subject: ObjectFilter::Clause(ObjectClause {
                                owner: ControllerFilter::You,
                                controller: ControllerFilter::Opponent,
                                ..ObjectClause::creature()
                            }),
                            mode: AttackTriggerMode::Individual {
                                capture_defender: false,
                            },
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "you cast an instant or sorcery spell with a single target" => {
                    return Ok((
                        TriggerEvent::SpellCast {
                            caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                            filter: SpellFilter::instant_or_sorcery().single_target(),
                            self_cast: false,
                            include_copy: false,
                            min_mana_value: None,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "~ or another creature you control leaves the battlefield without dying" => {
                    return Ok((
                        TriggerEvent::LeavesBattlefieldWithoutDying {
                            subject: ObjectFilter::creature().controlled().or_self(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "another creature you control leaves the battlefield without dying" => {
                    return Ok((
                        TriggerEvent::LeavesBattlefieldWithoutDying {
                            subject: ObjectFilter::creature()
                                .controlled()
                                .other_than_ability_source(),
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                "one or more other creatures you control leave the battlefield without dying" => {
                    return Ok((
                        TriggerEvent::LeavesBattlefieldWithoutDying {
                            subject: ObjectFilter::creature()
                                .controlled()
                                .other_than_ability_source(),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more creatures you control leave the battlefield without dying" => {
                    return Ok((
                        TriggerEvent::LeavesBattlefieldWithoutDying {
                            subject: ObjectFilter::creature().controlled(),
                        },
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "one or more creatures you control fight or become blocked" => {
                    return Ok((
                        TriggerEvent::AnyOf(vec![
                            TriggerEvent::StatusChanged {
                                subject: ObjectFilter::creature().controlled(),
                                kind: StatusTriggerKind::Fought,
                            },
                            TriggerEvent::BlocksOrBlocked {
                                subject: ObjectFilter::creature().controlled(),
                                kind: BlockTriggerKind::BecomesBlocked {
                                    capture_defender: false,
                                },
                            },
                        ]),
                        TriggerGrouping::OneOrMoreTogether,
                    ));
                }
                "an opponent is dealt damage by a red instant or sorcery spell you control or by a red planeswalker you control" =>
                {
                    return Ok((
                        TriggerEvent::DealsDamage {
                            source: ObjectFilter::Either(
                                ObjectClause::any()
                                    .any_types(CardTypeMask::INSTANT.union(CardTypeMask::SORCERY))
                                    .controlled()
                                    .with_color(ObjectColorFilter::has(Color::Red)),
                                ObjectClause::any()
                                    .any_types(CardTypeMask::PLANESWALKER)
                                    .controlled()
                                    .with_color(ObjectColorFilter::has(Color::Red)),
                            ),
                            recipient: DamageTriggerRecipient::Opponent,
                            combat_only: false,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                _ => {}
            }
            if let Some(after_play) = low.strip_prefix("you play ")
                && let Some((land_raw, spell_raw)) = after_play.split_once(" or cast ")
                && !low.contains(" from ")
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let land_flt = spellwake_oracle_compiler::filter::parse_object_filter(
                    land_raw.trim(),
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:unsupported-trigger-event")?;
                let spell_flt = spellwake_oracle_compiler::filter::parse_object_filter(
                    spell_raw.trim(),
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:unsupported-trigger-event")?;
                if land_flt.zone.is_some()
                    || !land_flt.extra_zones.is_empty()
                    || land_flt.any_spell
                    || spell_flt.zone.is_some()
                    || !spell_flt.extra_zones.is_empty()
                    || land_flt.any_types != AstCardTypeMask::LAND
                {
                    return Err("oracle:unsupported-trigger-event");
                }
                let land_obj = lower_object_filter(&land_flt, vocabulary)
                    .map_err(|_| "oracle:unsupported-trigger-event")?;
                let land_action = if land_obj == ObjectFilter::land() {
                    PlayerTriggerAction::LandPlayed
                } else {
                    PlayerTriggerAction::LandPlayedFiltered(land_obj)
                };
                let sf = lower_spell_filter(&spell_flt, vocabulary)
                    .map_err(|_| "oracle:unsupported-trigger-event")?;
                return Ok((
                    TriggerEvent::AnyOf(vec![
                        TriggerEvent::PlayerAction {
                            player: PlayerRelation::Controller,
                            action: land_action,
                        },
                        TriggerEvent::SpellCast {
                            caster: TriggerPlayerScope::Relation(PlayerRelation::Controller),
                            filter: sf,
                            self_cast: false,
                            include_copy: false,
                            min_mana_value: None,
                        },
                    ]),
                    TriggerGrouping::EachOccurrence,
                ));
            }
            if !low.contains(" from ")
                && !low.contains(" or ")
                && !low.contains(" exiled ")
                && let Some((play_rel, flt_raw)) = low
                    .strip_prefix("you play ")
                    .map(|r| (PlayerRelation::Controller, r.trim()))
                    .or_else(|| {
                        low.strip_prefix("an opponent plays ")
                            .map(|r| (PlayerRelation::Opponent, r.trim()))
                    })
                    .or_else(|| {
                        low.strip_prefix("a player plays ")
                            .map(|r| (PlayerRelation::AnyParticipant, r.trim()))
                    })
            {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                if let Ok(parsed) =
                    spellwake_oracle_compiler::filter::parse_object_filter(flt_raw, &compiler_vocab)
                    && parsed.zone.is_none()
                    && parsed.extra_zones.is_empty()
                    && !parsed.any_spell
                    && let Ok(obj) = lower_object_filter(&parsed, vocabulary)
                {
                    let is_pure_land = parsed.any_types == AstCardTypeMask::LAND
                        || (parsed.any_types.is_empty()
                            && matches!(
                                obj,
                                ObjectFilter::Clause(c) if matches!(c.subtypes, SubtypeFilter::Land(_))
                            ));
                    if is_pure_land {
                        let land_action = if obj == ObjectFilter::land() {
                            PlayerTriggerAction::LandPlayed
                        } else {
                            PlayerTriggerAction::LandPlayedFiltered(obj)
                        };
                        return Ok((
                            TriggerEvent::PlayerAction {
                                player: play_rel,
                                action: land_action,
                            },
                            TriggerGrouping::EachOccurrence,
                        ));
                    }
                }
            }
            if let Some(into_raw) = low.strip_prefix("a permanent you control transforms into ") {
                let compiler_vocab = cached_compiler_vocab(vocabulary);
                let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                    into_raw.trim(),
                    &compiler_vocab,
                )
                .map_err(|_| "oracle:unsupported-trigger-event")?;
                if parsed.zone.is_some() || !parsed.extra_zones.is_empty() || parsed.any_spell {
                    return Err("oracle:unsupported-trigger-event");
                }
                let subject = lower_object_filter(&parsed, vocabulary)
                    .map_err(|_| "oracle:unsupported-trigger-event")?
                    .controlled();
                return Ok((
                    TriggerEvent::StatusChanged {
                        subject,
                        kind: StatusTriggerKind::Transforms,
                    },
                    TriggerGrouping::EachOccurrence,
                ));
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
                | "spell_or_ability_shuffle_library"
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
                "~ becomes saddled" | "saddle" | "saddles" | "~ saddles a mount"
                | "saddles a mount" | "you saddle a mount" | "you saddle" => {
                    PlayerTriggerAction::Saddled
                }
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
                "earthbend" | "earthbends" | "you earthbend" => PlayerTriggerAction::Earthbent,
                "finish_voting" | "players finish voting" => PlayerTriggerAction::FinishedVoting,
                "own_spell_countered"
                | "a spell you've cast is countered"
                | "a spell you cast is countered"
                | "a spell you control is countered" => PlayerTriggerAction::OwnSpellCountered,
                "play a land" | "plays a land" | "you play a land" | "you play another land" => {
                    PlayerTriggerAction::LandPlayed
                }
                "you cycle" => PlayerTriggerAction::Cycled { self_only: false },
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
            let mut parts = Vec::new();
            expand_trigger_subject_parts(subject, &mut parts);
            if parts.is_empty() {
                return Err("oracle:unsupported-trigger-subject");
            }
            let mut sub_events = Vec::with_capacity(parts.len());
            for part in parts {
                if let SubjectRefAst::Player(p) = part {
                    if by_controller {
                        return Err("oracle:unsupported-targeted-by");
                    }
                    let rel = lower_player_relation(*p)?;
                    sub_events.push(TriggerEvent::PlayerTargeted {
                        player: rel,
                        by_opponent: opp,
                        spell_only,
                        instant_or_sorcery_only: inst_sorc,
                    });
                } else {
                    for subj in lower_trigger_subject_filters(part, vocabulary)? {
                        if by_controller {
                            sub_events.push(TriggerEvent::TargetedByController {
                                subject: subj,
                                spell_only,
                                instant_or_sorcery_only: inst_sorc,
                            });
                        } else {
                            sub_events.push(TriggerEvent::Targeted {
                                subject: subj,
                                by_opponent: opp,
                                spell_only,
                                instant_or_sorcery_only: inst_sorc,
                            });
                        }
                    }
                }
            }
            Ok((wrap_trigger_events_any_of(sub_events)?, grouping))
        }
        TriggerEventAst::PlayerOrPermanentBecomesTargeted {
            player,
            permanent_filter,
        } => {
            if permanent_filter.zone.is_some() || permanent_filter.any_spell {
                return Err("oracle:unsupported-trigger-event");
            }
            let rel = lower_player_relation(*player)?;
            let subject = if is_unrestricted_filter(permanent_filter) {
                ObjectFilter::permanent()
            } else {
                lower_object_filter(permanent_filter, vocabulary)?
            };
            Ok((
                TriggerEvent::AnyOf(vec![
                    TriggerEvent::PlayerTargeted {
                        player: rel,
                        by_opponent: false,
                        spell_only: false,
                        instant_or_sorcery_only: false,
                    },
                    TriggerEvent::Targeted {
                        subject,
                        by_opponent: false,
                        spell_only: false,
                        instant_or_sorcery_only: false,
                    },
                ]),
                TriggerGrouping::OneOrMoreByPlayer,
            ))
        }
        TriggerEventAst::CountersChanged {
            subject,
            counter_kind,
            added,
            by_you,
        } => {
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
                    || ck_low == "counter"
                    || ck_low == "counters"
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
            let mut subjects = Vec::new();
            lower_counters_changed_subjects_into(subject, vocabulary, &mut subjects)?;
            let sub_events = subjects
                .into_iter()
                .map(|subj| TriggerEvent::CountersChanged {
                    subject: subj,
                    kind,
                })
                .collect();
            Ok((
                wrap_trigger_events_any_of(sub_events)?,
                TriggerGrouping::EachOccurrence,
            ))
        }
        TriggerEventAst::TapStateChanged {
            subject,
            tapped: true,
            for_mana: true,
        } => {
            let mut parts = Vec::new();
            expand_trigger_subject_parts(subject, &mut parts);
            let mut sub_events = Vec::with_capacity(parts.len());
            for part in parts {
                let mut cur = part;
                while let SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } = cur
                {
                    cur = inner.as_ref();
                }
                if let SubjectRefAst::Group(f) | SubjectRefAst::Each(f) | SubjectRefAst::All(f) =
                    cur
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
                    sub_events.push(TriggerEvent::PlayerAction {
                        player: rel,
                        action: PlayerTriggerAction::ArtifactTappedForMana,
                    });
                    continue;
                }
                let subj_filters = lower_trigger_subject_filters(cur, vocabulary)?;
                let rel = match cur {
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
                for subj in subj_filters {
                    sub_events.push(TriggerEvent::PlayerAction {
                        player: rel,
                        action: PlayerTriggerAction::PermanentTappedForMana(subj),
                    });
                }
            }
            Ok((
                wrap_trigger_events_any_of(sub_events)?,
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
        }
        | TriggerEventAst::QualifiedAbilityActivated {
            player,
            source_filter,
            loyalty_only: false,
            nonmana_only: true,
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
        TriggerEventAst::BecomesAttached { subject, .. } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| {
                TriggerEvent::BecomesAttached { subject: subj }
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Exploits { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::Exploits {
                subject: subj,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
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
        TriggerEventAst::CardPlayed { player, filter } => {
            if is_unrestricted_filter(filter) {
                let rel = lower_player_relation(*player)?;
                return Ok((
                    TriggerEvent::AnyOf(vec![
                        TriggerEvent::PlayerAction {
                            player: rel,
                            action: PlayerTriggerAction::LandPlayed,
                        },
                        TriggerEvent::SpellCast {
                            caster: TriggerPlayerScope::Relation(rel),
                            filter: SpellFilter::any(),
                            self_cast: false,
                            include_copy: false,
                            min_mana_value: None,
                        },
                    ]),
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let excludes_land = (filter.forbidden_types.0 & AstCardTypeMask::LAND.0) != 0
                || (!filter.any_types.is_empty()
                    && (filter.any_types.0 & AstCardTypeMask::LAND.0) == 0);
            if !excludes_land {
                // LandPlayedFiltered matches the played land's battlefield appearance and does
                // not verify origin zones; fail closed on any zone qualifier or spell-only filter.
                if filter.zone.is_some() || !filter.extra_zones.is_empty() || filter.any_spell {
                    return Err("oracle:unsupported-trigger-event");
                }
                let rel = lower_player_relation(*player)?;
                let obj = lower_object_filter(filter, vocabulary)?;
                let is_pure_land = filter.any_types == AstCardTypeMask::LAND
                    || (filter.any_types.is_empty()
                        && matches!(
                            obj,
                            ObjectFilter::Clause(c) if matches!(c.subtypes, SubtypeFilter::Land(_))
                        ));
                let land_action = if obj == ObjectFilter::land() {
                    PlayerTriggerAction::LandPlayed
                } else {
                    PlayerTriggerAction::LandPlayedFiltered(obj)
                };
                if is_pure_land {
                    return Ok((
                        TriggerEvent::PlayerAction {
                            player: rel,
                            action: land_action,
                        },
                        TriggerGrouping::EachOccurrence,
                    ));
                }
                let sf = lower_spell_filter(filter, vocabulary)?;
                return Ok((
                    TriggerEvent::AnyOf(vec![
                        TriggerEvent::PlayerAction {
                            player: rel,
                            action: land_action,
                        },
                        TriggerEvent::SpellCast {
                            caster: TriggerPlayerScope::Relation(rel),
                            filter: sf,
                            self_cast: false,
                            include_copy: false,
                            min_mana_value: None,
                        },
                    ]),
                    TriggerGrouping::EachOccurrence,
                ));
            }
            let scope = match player {
                PlayerRefAst::EnchantedPlayer => TriggerPlayerScope::EnchantedPlayer,
                other => TriggerPlayerScope::Relation(lower_player_relation(*other)?),
            };
            let sf = lower_spell_filter(filter, vocabulary)?;
            Ok((
                TriggerEvent::SpellCast {
                    caster: scope,
                    filter: sf,
                    self_cast: false,
                    include_copy: false,
                    min_mana_value: None,
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
        TriggerEventAst::Crewed { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Crewed,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Exerted { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Exerted,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Explores { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::StatusChanged {
                subject: subj,
                kind: StatusTriggerKind::Explored,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::Connives { .. } => Ok((
            TriggerEvent::PlayerAction {
                player: PlayerRelation::Controller,
                action: PlayerTriggerAction::Connived,
            },
            TriggerGrouping::EachOccurrence,
        )),
        TriggerEventAst::UnlockDoor { subject } => Ok((
            lower_subject_trigger_event(subject, vocabulary, |subj| TriggerEvent::DoorUnlocked {
                subject: subj,
                fully_unlocked_only: false,
                door: None,
            })?,
            TriggerGrouping::EachOccurrence,
        )),
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
                    door: None,
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

fn parse_attack_count_word(raw: &str) -> Option<u8> {
    match raw.trim() {
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        "seven" | "7" => Some(7),
        "eight" | "8" => Some(8),
        "nine" | "9" => Some(9),
        "ten" | "10" => Some(10),
        other => other.parse::<u8>().ok().filter(|&n| (2..=20).contains(&n)),
    }
}

fn parse_attack_count_prefix(raw: &str) -> Option<(u8, &str)> {
    if let Some((num_raw, rest)) = raw.split_once(" or more ")
        && let Some(n) = parse_attack_count_word(num_raw)
    {
        return Some((n, rest.trim()));
    }
    if let Some(after) = raw.strip_prefix("at least ")
        && let Some((num_raw, rest)) = after.split_once(' ')
        && let Some(n) = parse_attack_count_word(num_raw)
    {
        return Some((n, rest.trim()));
    }
    None
}

fn parse_small_u16_count(raw: &str) -> Option<u16> {
    let t = raw.trim();
    if let Ok(n) = t.parse::<u16>() {
        return (n > 0).then_some(n);
    }
    match t {
        "one" => Some(1),
        "two" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        _ => None,
    }
}

fn try_lower_damage_threshold_trigger(low: &str) -> Option<(TriggerEvent, TriggerGrouping)> {
    if let Some((src_raw, rest)) = low.split_once(" deals ") {
        let source = match src_raw.trim() {
            "~" => ObjectFilter::self_object(),
            "a source you control" => ObjectFilter::any().controlled(),
            "another source you control" => {
                ObjectFilter::any().controlled().other_than_ability_source()
            }
            "a creature you control" => ObjectFilter::creature().controlled(),
            "another creature you control" => ObjectFilter::creature()
                .controlled()
                .other_than_ability_source(),
            "equipped creature" | "enchanted creature" => ObjectFilter::any().attached_to_self(),
            _ => return None,
        };
        let (rest_no_rec, recipient) = if let Some(r) = rest
            .strip_suffix(" to a permanent or player")
            .or_else(|| rest.strip_suffix(" to any target"))
        {
            (r.trim(), DamageTriggerRecipient::Any)
        } else if let Some(r) = rest.strip_suffix(" to a player") {
            (r.trim(), DamageTriggerRecipient::AnyPlayer)
        } else if let Some(r) = rest.strip_suffix(" to an opponent") {
            (r.trim(), DamageTriggerRecipient::Opponent)
        } else if let Some(r) = rest.strip_suffix(" to you") {
            (r.trim(), DamageTriggerRecipient::Controller)
        } else if let Some(r) = rest.strip_suffix(" to a creature") {
            (r.trim(), DamageTriggerRecipient::Creature)
        } else if let Some(r) = rest.strip_suffix(" to a battle") {
            (r.trim(), DamageTriggerRecipient::Battle)
        } else if rest.ends_with(" damage") {
            (rest.trim(), DamageTriggerRecipient::Any)
        } else {
            return None;
        };
        let (qty_part, combat_only) = if let Some(q) = rest_no_rec.strip_suffix(" combat damage") {
            (q.trim(), Some(true))
        } else if let Some(q) = rest_no_rec.strip_suffix(" noncombat damage") {
            (q.trim(), Some(false))
        } else {
            let q = rest_no_rec.strip_suffix(" damage")?;
            (q.trim(), None)
        };
        let (min_damage, exact) = if let Some(n_str) = qty_part.strip_prefix("exactly ") {
            (parse_small_u16_count(n_str)?, true)
        } else {
            let n_str = qty_part.strip_suffix(" or more")?;
            (parse_small_u16_count(n_str)?, false)
        };
        return Some((
            TriggerEvent::DealsAtLeastDamage {
                source,
                recipient,
                min_damage,
                exact,
                combat_only,
            },
            TriggerGrouping::EachOccurrence,
        ));
    }
    if let Some(rest) = low.strip_prefix("an opponent is dealt ")
        && let Some(n_str) = rest.strip_suffix(" or more damage by a single source")
        && let Some(min_damage) = parse_small_u16_count(n_str)
    {
        return Some((
            TriggerEvent::DealsAtLeastDamage {
                source: ObjectFilter::any(),
                recipient: DamageTriggerRecipient::Opponent,
                min_damage,
                exact: false,
                combat_only: None,
            },
            TriggerGrouping::EachOccurrence,
        ));
    }
    if let Some(rest) = low.strip_prefix("~ is dealt ")
        && let Some(n_str) = rest.strip_suffix(" or more damage")
        && let Some(min_damage) = parse_small_u16_count(n_str)
    {
        return Some((
            TriggerEvent::DealsAtLeastDamage {
                source: ObjectFilter::any(),
                recipient: DamageTriggerRecipient::SelfObject,
                min_damage,
                exact: false,
                combat_only: None,
            },
            TriggerGrouping::EachOccurrence,
        ));
    }
    None
}

fn expand_trigger_subject_parts<'a>(subject: &'a SubjectRefAst, out: &mut Vec<&'a SubjectRefAst>) {
    match subject {
        SubjectRefAst::Compound(items) => {
            for item in items {
                expand_trigger_subject_parts(item, out);
            }
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(1),
        } => {
            for item in options {
                expand_trigger_subject_parts(item, out);
            }
        }
        other => out.push(other),
    }
}

fn wrap_trigger_events_any_of(events: Vec<TriggerEvent>) -> Result<TriggerEvent, &'static str> {
    let mut flat = Vec::with_capacity(events.len());
    for ev in events {
        if let TriggerEvent::AnyOf(inner) = ev {
            flat.extend(inner);
        } else {
            flat.push(ev);
        }
    }
    match flat.len() {
        0 => Err("oracle:empty-trigger-events"),
        1 => flat.into_iter().next().ok_or("oracle:empty-trigger-events"),
        _ => Ok(TriggerEvent::AnyOf(flat)),
    }
}

fn lower_trigger_subject_filters_into(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
    out: &mut Vec<ObjectFilter>,
) -> Result<(), &'static str> {
    match subject {
        SubjectRefAst::Compound(items) => {
            if items.is_empty() {
                return Err("oracle:unsupported-trigger-subject");
            }
            for item in items {
                lower_trigger_subject_filters_into(item, vocabulary, out)?;
            }
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(1),
        } => {
            if options.is_empty() {
                return Err("oracle:unsupported-trigger-subject");
            }
            for item in options {
                lower_trigger_subject_filters_into(item, vocabulary, out)?;
            }
            Ok(())
        }
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => lower_trigger_subject_filters_into(inner, vocabulary, out),
        other => {
            out.push(lower_trigger_subject_filter(other, vocabulary)?);
            Ok(())
        }
    }
}

fn lower_trigger_subject_filters(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<Vec<ObjectFilter>, &'static str> {
    let mut out = Vec::new();
    lower_trigger_subject_filters_into(subject, vocabulary, &mut out)?;
    if out.is_empty() {
        return Err("oracle:unsupported-trigger-subject");
    }
    Ok(out)
}

fn lower_subject_trigger_event<F>(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
    build: F,
) -> Result<TriggerEvent, &'static str>
where
    F: FnMut(ObjectFilter) -> TriggerEvent,
{
    let filters = lower_trigger_subject_filters(subject, vocabulary)?;
    let events: Vec<TriggerEvent> = filters.into_iter().map(build).collect();
    wrap_trigger_events_any_of(events)
}

fn lower_counters_changed_subjects_into(
    subject: &SubjectRefAst,
    vocabulary: &CreatureTypeVocabulary,
    out: &mut Vec<ObjectFilter>,
) -> Result<(), &'static str> {
    match subject {
        SubjectRefAst::Compound(items) => {
            if items.is_empty() {
                return Err("oracle:unsupported-trigger-subject");
            }
            for item in items {
                lower_counters_changed_subjects_into(item, vocabulary, out)?;
            }
            Ok(())
        }
        SubjectRefAst::TargetOneOf {
            options,
            count: TargetCountAst::Exact(1),
        } => {
            if options.is_empty() {
                return Err("oracle:unsupported-trigger-subject");
            }
            for item in options {
                lower_counters_changed_subjects_into(item, vocabulary, out)?;
            }
            Ok(())
        }
        SubjectRefAst::AnyTarget => {
            out.push(ObjectFilter::any());
            Ok(())
        }
        SubjectRefAst::TargetObject { filter, .. } => {
            out.push(lower_object_filter(filter, vocabulary)?);
            Ok(())
        }
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => lower_counters_changed_subjects_into(inner, vocabulary, out),
        other => lower_trigger_subject_filters_into(other, vocabulary, out),
    }
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
        SubjectRefAst::Anaphora(raw) if raw.trim().eq_ignore_ascii_case("your commander") => {
            Ok(ObjectFilter::Clause(ObjectClause {
                owner: ControllerFilter::You,
                flags: ObjectFilterFlags::COMMANDER_ONLY,
                ..ObjectClause::any()
            }))
        }
        SubjectRefAst::Anaphora(raw)
            if raw.trim().eq_ignore_ascii_case("a commander you control")
                || raw
                    .trim()
                    .eq_ignore_ascii_case("your commander you control") =>
        {
            Ok(ObjectFilter::Clause(ObjectClause {
                controller: ControllerFilter::You,
                flags: ObjectFilterFlags::COMMANDER_ONLY,
                ..ObjectClause::any()
            }))
        }
        SubjectRefAst::InZone {
            subject: inner,
            zone: ZoneFilter::Battlefield,
        } => lower_trigger_subject_filter(inner, vocabulary),
        _ => Err("oracle:unsupported-trigger-subject"),
    }
}

fn clause_supported_by_zone_move_record(clause: ObjectClause) -> bool {
    let ctrl_ok = matches!(
        clause.controller,
        ControllerFilter::Any
            | ControllerFilter::You
            | ControllerFilter::Opponent
            | ControllerFilter::NotYou
            | ControllerFilter::OwnerYou
            | ControllerFilter::OwnerOpponent
            | ControllerFilter::Player(_)
    );
    let subtype_ok = matches!(
        clause.subtypes,
        SubtypeFilter::Any
            | SubtypeFilter::Has(_)
            | SubtypeFilter::Lacks(_)
            | SubtypeFilter::AnyOf(_)
            | SubtypeFilter::NoneOf(_)
            | SubtypeFilter::Artifact(_)
    );
    ctrl_ok
        && subtype_ok
        && clause.flags == ObjectFilterFlags::EMPTY
        && clause.source_relation == SourceRelation::Any
        && clause.combat == CombatFilter::Any
        && clause.power == PowerFilter::Any
        && clause.toughness == ToughnessFilter::Any
        && clause.mana_value == ManaValueFilter::Any
        && clause.keyword.is_none()
        && clause.forbidden_keyword.is_none()
        && clause.tapped.is_none()
        && clause.token.is_none()
        && clause.exact_name.is_none()
        && clause.required_counter_kind.is_none()
        && clause.forbidden_counter_kind.is_none()
}

fn filter_supported_by_zone_move_record(filter: &ObjectFilter) -> bool {
    match filter {
        ObjectFilter::Clause(c) => clause_supported_by_zone_move_record(*c),
        ObjectFilter::Either(a, b) => {
            clause_supported_by_zone_move_record(*a) && clause_supported_by_zone_move_record(*b)
        }
    }
}

fn filter_supported_by_cast_spell_record(filter: &ObjectFilter) -> bool {
    let clause_ok = |c: ObjectClause| {
        clause_supported_by_zone_move_record(c)
            && !matches!(c.subtypes, SubtypeFilter::Artifact(_))
            && c.controller == ControllerFilter::Any
            && c.owner == ControllerFilter::Any
    };
    match filter {
        ObjectFilter::Clause(c) => clause_ok(*c),
        ObjectFilter::Either(a, b) => clause_ok(*a) && clause_ok(*b),
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

fn is_self_anaphora_label(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "~" | "this creature"
            | "this permanent"
            | "this artifact"
            | "this enchantment"
            | "this land"
            | "this planeswalker"
            | "this card"
            | "this battle"
            | "this vehicle"
    )
}

fn try_lower_specialized_trigger_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<TriggerCondition> {
    match cond {
        ConditionAst::IsYourTurn | ConditionAst::IsPlayersTurn(PlayerRefAst::You) => {
            Some(TriggerCondition::ControllersTurn)
        }
        ConditionAst::IsPlayersTurn(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent) => {
            Some(TriggerCondition::OpponentsTurn)
        }
        ConditionAst::Not(inner)
            if matches!(
                inner.as_ref(),
                ConditionAst::IsYourTurn | ConditionAst::IsPlayersTurn(PlayerRefAst::You)
            ) =>
        {
            Some(TriggerCondition::OpponentsTurn)
        }
        ConditionAst::Marker(GameMarker::DuringStep { player, step }) => {
            let step_cond = match step {
                TurnStepAst::UntapStep => TriggerCondition::DuringStep(Step::Untap),
                TurnStepAst::Upkeep => TriggerCondition::DuringStep(Step::Upkeep),
                TurnStepAst::DrawStep => TriggerCondition::DuringStep(Step::Draw),
                TurnStepAst::PrecombatMain => TriggerCondition::DuringStep(Step::PrecombatMain),
                TurnStepAst::PostcombatMain => TriggerCondition::DuringStep(Step::PostcombatMain),
                TurnStepAst::MainPhase => TriggerCondition::Or(
                    Box::new(TriggerCondition::DuringStep(Step::PrecombatMain)),
                    Box::new(TriggerCondition::DuringStep(Step::PostcombatMain)),
                ),
                TurnStepAst::BeginningOfCombat => {
                    TriggerCondition::DuringStep(Step::BeginningCombat)
                }
                TurnStepAst::EndOfCombat => TriggerCondition::DuringStep(Step::EndCombat),
                TurnStepAst::EndStep => TriggerCondition::DuringStep(Step::End),
                TurnStepAst::CleanupStep => TriggerCondition::DuringStep(Step::Cleanup),
            };
            match player {
                PlayerRefAst::You => Some(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(step_cond),
                )),
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    Some(TriggerCondition::And(
                        Box::new(TriggerCondition::OpponentsTurn),
                        Box::new(step_cond),
                    ))
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => Some(step_cond),
                _ => None,
            }
        }
        ConditionAst::IsPhaseOrStep {
            player,
            phase,
            first_only: false,
        } => {
            let step_cond = match phase.trim().to_ascii_lowercase().as_str() {
                "untap" | "untap_step" => TriggerCondition::DuringStep(Step::Untap),
                "upkeep" | "upkeep_step" => TriggerCondition::DuringStep(Step::Upkeep),
                "draw" | "draw_step" => TriggerCondition::DuringStep(Step::Draw),
                "precombat_main" | "precombat_main_phase" => {
                    TriggerCondition::DuringStep(Step::PrecombatMain)
                }
                "begin_combat" | "beginning_of_combat" | "beginning_combat" => {
                    TriggerCondition::DuringStep(Step::BeginningCombat)
                }
                "declare_attackers" | "declare_attackers_step" => {
                    TriggerCondition::DuringStep(Step::DeclareAttackers)
                }
                "declare_blockers" | "declare_blockers_step" => {
                    TriggerCondition::DuringStep(Step::DeclareBlockers)
                }
                "combat_damage" | "combat_damage_step" => {
                    TriggerCondition::DuringStep(Step::CombatDamage)
                }
                "end_combat" | "end_of_combat" | "end_combat_step" => {
                    TriggerCondition::DuringStep(Step::EndCombat)
                }
                "postcombat_main" | "postcombat_main_phase" => {
                    TriggerCondition::DuringStep(Step::PostcombatMain)
                }
                "main_phase" | "main" => TriggerCondition::Or(
                    Box::new(TriggerCondition::DuringStep(Step::PrecombatMain)),
                    Box::new(TriggerCondition::DuringStep(Step::PostcombatMain)),
                ),
                "combat" | "combat_phase" => combat_steps_condition(),
                "end_step" | "end" => TriggerCondition::DuringStep(Step::End),
                "cleanup" | "cleanup_step" => TriggerCondition::DuringStep(Step::Cleanup),
                _ => return None,
            };
            match player {
                None | Some(PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer) => Some(step_cond),
                Some(PlayerRefAst::You) => Some(TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(step_cond),
                )),
                Some(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent) => {
                    Some(TriggerCondition::And(
                        Box::new(TriggerCondition::OpponentsTurn),
                        Box::new(step_cond),
                    ))
                }
                _ => None,
            }
        }
        ConditionAst::Delirium => Some(TriggerCondition::Delirium),
        ConditionAst::Metalcraft => Some(TriggerCondition::Metalcraft),
        ConditionAst::Threshold => Some(TriggerCondition::Threshold),
        ConditionAst::Revolt => Some(TriggerCondition::PermanentLeftBattlefieldThisTurn),
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
        ConditionAst::Marker(GameMarker::NoMonarch) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::NoMonarch),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::CounterCompare {
            subject: subj,
            counter_kind,
            op: CompareOp::Eq,
            value: 0,
        } if matches!(
            subj.as_ref(),
            SubjectRefAst::Player(PlayerRefAst::ThatPlayer)
        ) && counter_kind
            .trim()
            .eq_ignore_ascii_case("cards in hand and ~ has two or more quest") =>
        {
            let quest = parse_counter_kind("quest")?;
            Some(TriggerCondition::And(
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(
                        ResolvingPlayer::UniqueRelatedPlayer,
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                }))),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::counters(
                        spellwake_engine::program::CounterSubjectRef::TriggerSource,
                        Some(quest),
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                }))),
            ))
        }
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
        ConditionAst::HandCount {
            player:
                p @ (PlayerRefAst::ThatPlayer
                | PlayerRefAst::DefendingPlayer
                | PlayerRefAst::ActivePlayer
                | PlayerRefAst::EnchantedPlayer),
            op,
            value,
        } => {
            let rp = match p {
                PlayerRefAst::DefendingPlayer => ResolvingPlayer::DefendingPlayer,
                PlayerRefAst::ActivePlayer => ResolvingPlayer::ActivePlayer,
                PlayerRefAst::EnchantedPlayer => ResolvingPlayer::EnchantedPlayer,
                _ => ResolvingPlayer::UniqueRelatedPlayer,
            };
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerHandSize(rp)),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })))
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a) if is_self_anaphora_label(a)
                ) =>
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
                "suspected" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::SUSPECTED,
                        ..ObjectClause::any()
                    }),
                ))),
                "monstrous" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::MONSTROUS,
                        ..ObjectClause::any()
                    }),
                ))),
                "goaded" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::GOADED,
                        ..ObjectClause::any()
                    }),
                ))),
                "exerted" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::EXERTED,
                        ..ObjectClause::any()
                    }),
                ))),
                "kicked" => Some(TriggerCondition::SourceMatches(Box::new(
                    ObjectFilter::Clause(ObjectClause {
                        flags: ObjectFilterFlags::KICKED,
                        ..ObjectClause::any()
                    }),
                ))),
                "solved" => Some(TriggerCondition::SourceIsSolved),
                "unsolved" => Some(TriggerCondition::SourceIsUnsolved),
                "renowned" => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::UnrenownedSource,
                ))),
                "unrenowned" | "not renowned" => Some(TriggerCondition::UnrenownedSource),
                "not_owned_by_you" | "not owned by you" | "not_owned_by_controller" => {
                    Some(TriggerCondition::SourceNotOwnedByController)
                }
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
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_))
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a)
                        if matches!(
                            a.trim().to_ascii_lowercase().as_str(),
                            "it" | "that creature"
                                | "that permanent"
                                | "that artifact"
                                | "that enchantment"
                                | "that land"
                                | "that card"
                                | "that spell"
                                | "that token"
                                | "that planeswalker"
                                | "that battle"
                                | "that equipment"
                                | "that aura"
                                | "that vehicle"
                                | "that mount"
                                | "that saga"
                        )
                ) =>
        {
            let match_ref = if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) {
                MatchObjectRef::AttachedHost
            } else {
                MatchObjectRef::TriggerSubject
            };
            let filter = match status.trim().to_ascii_lowercase().as_str() {
                "tapped" => Some(ObjectFilter::Clause(ObjectClause {
                    tapped: Some(true),
                    ..ObjectClause::any()
                })),
                "untapped" => Some(ObjectFilter::Clause(ObjectClause {
                    tapped: Some(false),
                    ..ObjectClause::any()
                })),
                "attacking" => Some(ObjectFilter::Clause(ObjectClause {
                    combat: CombatFilter::Attacking,
                    ..ObjectClause::any()
                })),
                "blocking" => Some(ObjectFilter::Clause(ObjectClause {
                    combat: CombatFilter::Blocking,
                    ..ObjectClause::any()
                })),
                "attacking or blocking" => Some(ObjectFilter::Clause(ObjectClause {
                    combat: CombatFilter::AttackingOrBlocking,
                    ..ObjectClause::any()
                })),
                "blocked" => Some(ObjectFilter::Clause(ObjectClause {
                    combat: CombatFilter::Blocked,
                    ..ObjectClause::any()
                })),
                "unblocked" => Some(ObjectFilter::Clause(ObjectClause {
                    combat: CombatFilter::Unblocked,
                    ..ObjectClause::any()
                })),
                "enchanted" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::ENCHANTED,
                    ..ObjectClause::any()
                })),
                "equipped" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::EQUIPPED,
                    ..ObjectClause::any()
                })),
                "modified" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::MODIFIED,
                    ..ObjectClause::any()
                })),
                "face down" | "face-down" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::FACE_DOWN,
                    ..ObjectClause::any()
                })),
                "suspected" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::SUSPECTED,
                    ..ObjectClause::any()
                })),
                "renowned" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::RENOWNED,
                    ..ObjectClause::any()
                })),
                "saddled" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::SADDLED,
                    ..ObjectClause::any()
                })),
                "monstrous" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::MONSTROUS,
                    ..ObjectClause::any()
                })),
                "goaded" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::GOADED,
                    ..ObjectClause::any()
                })),
                "exerted" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::EXERTED,
                    ..ObjectClause::any()
                })),
                "kicked" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::KICKED,
                    ..ObjectClause::any()
                })),
                "solved" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::SOLVED,
                    ..ObjectClause::any()
                })),
                "prepared" => Some(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::PREPARED,
                    ..ObjectClause::any()
                })),
                other => {
                    if let Some(flt_raw) = other.strip_prefix("is:") {
                        let compiler_vocab = cached_compiler_vocab(vocabulary);
                        let parsed = spellwake_oracle_compiler::filter::parse_object_filter(
                            flt_raw.trim(),
                            &compiler_vocab,
                        )
                        .ok()?;
                        let obj = lower_object_filter(&parsed, vocabulary).ok()?;
                        Some(obj)
                    } else {
                        None
                    }
                }
            }?;
            let rc = ResolutionCondition {
                left: ValueExpr::ObjectMatches {
                    subject: match_ref,
                    filter: Box::new(filter),
                },
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            };
            rc.is_trigger_world_evaluable()
                .then(|| TriggerCondition::Eval(Box::new(rc)))
        }
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            || matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a) if is_self_anaphora_label(a)
            ) =>
        {
            let kind = parse_counter_kind(counter_kind)?;
            match (op, *value) {
                (CompareOp::Ge, v) if v > 0 => Some(TriggerCondition::SourceCounterAtLeast {
                    kind,
                    minimum: u8::try_from(v).ok()?,
                }),
                (CompareOp::Eq, 0) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::SourceCounterAtLeast { kind, minimum: 1 },
                ))),
                (CompareOp::Eq, v) if v > 0 => {
                    let min = u8::try_from(v).ok()?;
                    let over = v.checked_add(1).and_then(|n| u8::try_from(n).ok())?;
                    Some(TriggerCondition::And(
                        Box::new(TriggerCondition::SourceCounterAtLeast { kind, minimum: min }),
                        Box::new(TriggerCondition::Not(Box::new(
                            TriggerCondition::SourceCounterAtLeast {
                                kind,
                                minimum: over,
                            },
                        ))),
                    ))
                }
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
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_))
            || matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a)
                    if matches!(
                        a.trim().to_ascii_lowercase().as_str(),
                        "it" | "that creature"
                            | "that permanent"
                            | "that artifact"
                            | "that enchantment"
                            | "that land"
                            | "that token"
                            | "that planeswalker"
                            | "that battle"
                            | "that equipment"
                            | "that aura"
                            | "that vehicle"
                            | "that mount"
                            | "that saga"
                    )
            ) =>
        {
            let kind = parse_counter_kind(counter_kind)?;
            if !kind.is_permanent() {
                return None;
            }
            let counter_subject = if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) {
                spellwake_engine::program::CounterSubjectRef::AttachedHost
            } else {
                spellwake_engine::program::CounterSubjectRef::TriggerSubject
            };
            let rc = ResolutionCondition {
                left: ValueExpr::CounterCount {
                    subject: counter_subject,
                    kind: Some(kind),
                },
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            };
            rc.is_trigger_world_evaluable()
                .then(|| TriggerCondition::Eval(Box::new(rc)))
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
        ConditionAst::LifeCompare {
            player: PlayerRefAst::EachPlayer,
            op,
            value,
        } if *value >= 0 => match op {
            CompareOp::Le => Some(TriggerCondition::And(
                Box::new(TriggerCondition::ControllerLifeAtMost(
                    u16::try_from(*value).ok()?,
                )),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                    comparison: Comparison::LessOrEqual,
                    right: ValueExpr::literal(i64::from(*value)),
                }))),
            )),
            CompareOp::Lt if *value > 0 => Some(TriggerCondition::And(
                Box::new(TriggerCondition::ControllerLifeAtMost(
                    u16::try_from(*value - 1).ok()?,
                )),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                    comparison: Comparison::Less,
                    right: ValueExpr::literal(i64::from(*value)),
                }))),
            )),
            CompareOp::Ge => Some(TriggerCondition::And(
                Box::new(TriggerCondition::ControllerLifeAtLeast(
                    u16::try_from(*value).ok()?,
                )),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*value)),
                }))),
            )),
            CompareOp::Gt => Some(TriggerCondition::And(
                Box::new(TriggerCondition::ControllerLifeAtLeast(
                    value.checked_add(1).and_then(|n| u16::try_from(n).ok())?,
                )),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                    comparison: Comparison::Greater,
                    right: ValueExpr::literal(i64::from(*value)),
                }))),
            )),
            _ => None,
        },
        ConditionAst::LifeCompare {
            player:
                p @ (PlayerRefAst::ThatPlayer
                | PlayerRefAst::DefendingPlayer
                | PlayerRefAst::ActivePlayer
                | PlayerRefAst::EnchantedPlayer),
            op,
            value,
        } => {
            let rp = match p {
                PlayerRefAst::DefendingPlayer => ResolvingPlayer::DefendingPlayer,
                PlayerRefAst::ActivePlayer => ResolvingPlayer::ActivePlayer,
                PlayerRefAst::EnchantedPlayer => ResolvingPlayer::EnchantedPlayer,
                _ => ResolvingPlayer::UniqueRelatedPlayer,
            };
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp)),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })))
        }
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
            player: PlayerRefAst::AnyPlayer,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            let rc = ResolutionCondition {
                left: ValueExpr::count_battlefield(obj),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            };
            rc.is_trigger_world_evaluable()
                .then(|| TriggerCondition::Eval(Box::new(rc)))
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
            player:
                p @ (PlayerRefAst::DefendingPlayer
                | PlayerRefAst::ThatPlayer
                | PlayerRefAst::ActivePlayer
                | PlayerRefAst::EnchantedPlayer),
            filter,
            op,
            value,
        } if matches!(
            (p, filter.controller),
            (
                PlayerRefAst::DefendingPlayer,
                None | Some(ControllerRelation::DefendingPlayer)
            ) | (
                PlayerRefAst::ThatPlayer,
                None | Some(ControllerRelation::ThatPlayer)
            ) | (
                PlayerRefAst::ActivePlayer,
                None | Some(ControllerRelation::ActivePlayer)
            ) | (PlayerRefAst::EnchantedPlayer, None)
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
            let rp = match p {
                PlayerRefAst::DefendingPlayer => ResolvingPlayer::DefendingPlayer,
                PlayerRefAst::ActivePlayer => ResolvingPlayer::ActivePlayer,
                PlayerRefAst::EnchantedPlayer => ResolvingPlayer::EnchantedPlayer,
                _ => ResolvingPlayer::UniqueRelatedPlayer,
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
        ConditionAst::GraveyardCount {
            player:
                p @ (PlayerRefAst::You
                | PlayerRefAst::ThatPlayer
                | PlayerRefAst::DefendingPlayer
                | PlayerRefAst::ActivePlayer
                | PlayerRefAst::EnchantedPlayer),
            op,
            value,
        } => {
            let rp = match p {
                PlayerRefAst::You => ResolvingPlayer::Controller,
                PlayerRefAst::DefendingPlayer => ResolvingPlayer::DefendingPlayer,
                PlayerRefAst::ActivePlayer => ResolvingPlayer::ActivePlayer,
                PlayerRefAst::EnchantedPlayer => ResolvingPlayer::EnchantedPlayer,
                _ => ResolvingPlayer::UniqueRelatedPlayer,
            };
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerGraveyardSize(rp)),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })))
        }
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
                (CompareOp::Gt, v) => Some(TriggerCondition::GraveyardMatching {
                    filter: Box::new(obj),
                    minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                }),
                (CompareOp::Eq, 0) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: 1,
                    },
                ))),
                (CompareOp::Eq, v) if v > 0 => {
                    let min = u8::try_from(v).ok()?;
                    let over = v.checked_add(1).and_then(|n| u8::try_from(n).ok())?;
                    Some(TriggerCondition::And(
                        Box::new(TriggerCondition::GraveyardMatching {
                            filter: Box::new(obj),
                            minimum: min,
                        }),
                        Box::new(TriggerCondition::Not(Box::new(
                            TriggerCondition::GraveyardMatching {
                                filter: Box::new(obj),
                                minimum: over,
                            },
                        ))),
                    ))
                }
                (CompareOp::Le, v) => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: v.checked_add(1).and_then(|n| u8::try_from(n).ok())?,
                    },
                ))),
                (CompareOp::Lt, v) if v > 0 => Some(TriggerCondition::Not(Box::new(
                    TriggerCondition::GraveyardMatching {
                        filter: Box::new(obj),
                        minimum: u8::try_from(v).ok()?,
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
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count }
            if *min_count > 0
                && filter_only_has(
                    filter,
                    FilterAllow {
                        types: true,
                        controller: true,
                        ..FilterAllow::NONE
                    },
                )
                && matches!(
                    filter.controller,
                    None | Some(ControllerRelation::You | ControllerRelation::YourTeam)
                )
                && filter.forbidden_types.is_empty() =>
        {
            if filter.any_types.is_empty() {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredPermanentThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                })))
            } else if filter.any_types == AstCardTypeMask::CREATURE {
                if *min_count == 1 {
                    Some(TriggerCondition::CreatureEnteredUnderControlThisTurn)
                } else {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::CreatureEnteredUnderControlThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    })))
                }
            } else if let Ok(mut types) = card_type_mask_to_set(filter.any_types)
                && types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredTypeThisTurn(kind)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                })))
            } else {
                None
            }
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.is_empty() => Some(TriggerCondition::ControllerCastSpellsThisTurn(1)),
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => match action
            .trim()
            .trim_end_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
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
            "you don't own it"
            | "you don't own ~"
            | "you don't own this creature"
            | "you don't own this permanent"
            | "not_owned_by_controller" => Some(TriggerCondition::SourceNotOwnedByController),
            "your team gained life this turn" => {
                Some(TriggerCondition::ControllerGainedLifeThisTurn)
            }
            "no permanents left the battlefield this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Not(Box::new(TriggerCondition::Or(
                    Box::new(TriggerCondition::PermanentLeftBattlefieldThisTurn),
                    Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::PermanentLeft,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    }))),
                ))))
            }
            "a creature died under an opponent's control this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(ObjectFilter::creature().opposing()),
                            from_battlefield_only: true,
                            controller: None,
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an enchantment was put into your graveyard from the battlefield this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(ObjectFilter::Clause(ObjectClause {
                                owner: ControllerFilter::You,
                                ..ObjectClause::enchantment()
                            })),
                            from_battlefield_only: true,
                            controller: None,
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a land you controlled was put into a graveyard from the battlefield this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(ObjectFilter::land()),
                            from_battlefield_only: true,
                            controller: Some(ResolvingPlayer::Controller),
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "that player controls more lands than each other player" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerControlsMost {
                        player: ResolvingPlayer::UniqueRelatedPlayer,
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::land()),
                        allow_tie: false,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "no creatures died this turn" => {
                Some(TriggerCondition::Not(Box::new(TriggerCondition::Morbid)))
            }
            "an opponent is poisoned" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::AnyOpponentPoisoned),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you've activated a loyalty ability this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::LoyaltyActivatedThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you didn't play a land this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                })))
            }
            "you didn't lose life this turn" => Some(TriggerCondition::Not(Box::new(
                TriggerCondition::ControllerLostLifeThisTurn,
            ))),
            "you've drawn more than one card this turn" => {
                Some(TriggerCondition::ControllerDrewCardsThisTurn(2))
            }
            "one or more cards were put into exile this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsExiledThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "~ attacked or blocked this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "~ was dealt damage this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::DamageDealtThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "~ dealt damage to an opponent this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DamageDealtThisTurn {
                        subject: ValueObjectRef::AbilitySource,
                        to_opponent_only: true,
                        combat_only: false,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you're the defending player" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsDefending),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you've rolled three or more dice this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DiceRolledThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(3),
                })))
            }
            "an opponent has been dealt noncombat damage this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: true,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you have a card in hand" => Some(TriggerCondition::HandSizeAtLeast(1)),
            "~ is your ring-bearer" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::IsRingBearer),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "your library has no cards in it" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                        ResolvingPlayer::Controller,
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                })))
            }
            "an enchantment is on the battlefield" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::enchantment()),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "ten or more lands are on the battlefield" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::land()),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(10),
                })))
            }
            "you have less life than an opponent" => Some(TriggerCondition::OpponentHasMoreLife),
            "you put a counter on a creature this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CountersPlacedOnCreaturesThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you control more creatures than attacking player" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::creature().controlled()),
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::ActivePlayer,
                        ObjectFilter::creature(),
                    ),
                })))
            }
            "you control more lands than attacking player" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::land().controlled()),
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::ActivePlayer,
                        ObjectFilter::land(),
                    ),
                })))
            }
            "you didn't activate a loyalty ability of a planeswalker this turn" => {
                Some(TriggerCondition::NoLoyaltyActivationThisTurn)
            }
            "there are five colors among permanents you control" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::DistinctColors {
                        filter: Box::new(ObjectFilter::permanent().controlled()),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(5),
                })))
            }
            "~ is in your graveyard or on the battlefield"
            | "~ is on the battlefield or in your graveyard" => Some(TriggerCondition::Or(
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::InZone(
                        spellwake_engine::program::SourceZoneKind::Graveyard,
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                }))),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::InZone(
                        spellwake_engine::program::SourceZoneKind::Battlefield,
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                }))),
            )),
            "~ is still on the battlefield" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::InZone(
                        spellwake_engine::program::SourceZoneKind::Battlefield,
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                })))
            }
            "~ is enchanted, equipped, and has a counter on it" => Some(
                TriggerCondition::SourceMatches(Box::new(ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::ENCHANTED
                        .union(ObjectFilterFlags::EQUIPPED)
                        .union(ObjectFilterFlags::WITH_ANY_COUNTER),
                    ..ObjectClause::any()
                }))),
            ),
            "a player other than you lost life this turn" => {
                Some(TriggerCondition::OpponentLostLifeAtLeast(1))
            }
            "they control fewer lands than you" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::UniqueRelatedPlayer,
                        ObjectFilter::land(),
                    ),
                    comparison: Comparison::Less,
                    right: ValueExpr::count_battlefield(ObjectFilter::land().controlled()),
                })))
            }
            "they control two or more basic lands" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::UniqueRelatedPlayer,
                        ObjectFilter::Clause(ObjectClause {
                            required_supertypes: SupertypeMask::BASIC,
                            ..ObjectClause::land()
                        }),
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                })))
            }
            "they control a commander" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::UniqueRelatedPlayer,
                        ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::COMMANDER_ONLY,
                            ..ObjectClause::any()
                        }),
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "at least one creature is blocking ~" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::BlockingSource,
                        ..ObjectClause::creature()
                    })),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "exactly one creature is attacking" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::Attacking,
                        ..ObjectClause::creature()
                    })),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                })))
            }
            "enchanted creature is attacking alone" => Some(TriggerCondition::And(
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::AttachedHost,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            combat: CombatFilter::Attacking,
                            ..ObjectClause::creature()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }))),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::count_battlefield(ObjectFilter::Clause(ObjectClause {
                        combat: CombatFilter::Attacking,
                        ..ObjectClause::creature()
                    })),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                }))),
            )),
            "three or more creatures attacked this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::AttackedMatchingThisTurn {
                        players: AffectedPlayers::EachPlayer,
                        filter: Box::new(ObjectFilter::creature()),
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(3),
                })))
            }
            "that creature has greater power or toughness than ~" => Some(TriggerCondition::Or(
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::TriggerSubject,
                        stat: ObjectStatKind::Power,
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::AbilitySource,
                        stat: ObjectStatKind::Power,
                    },
                }))),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::TriggerSubject,
                        stat: ObjectStatKind::Toughness,
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::ObjectStat {
                        subject: ValueObjectRef::AbilitySource,
                        stat: ObjectStatKind::Toughness,
                    },
                }))),
            )),
            "it was the second spell you cast this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(2),
                })))
            }
            "you cast it from anywhere other than your hand" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::CastFromOutsideHand),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "that player had another land enter the battlefield under their control this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(
                        ResolvingPlayer::UniqueRelatedPlayer,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(2),
                })))
            }
            "that card has the chosen name" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::MATCHES_CHOSEN_NAME,
                            ..ObjectClause::any()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "it was dealt damage this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN,
                            ..ObjectClause::any()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "its mana value is even" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::EVEN_MANA_VALUE,
                            ..ObjectClause::any()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "its mana value is odd" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::ODD_MANA_VALUE,
                            ..ObjectClause::any()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "it entered under your control" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::any().controlled()),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "no cards are in that graveyard" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerGraveyardSize(
                        ResolvingPlayer::UniqueRelatedPlayer,
                    )),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                })))
            }
            "that land was nonbasic" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            forbidden_supertypes: SupertypeMask::BASIC,
                            ..ObjectClause::land()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "that creature attacked this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::ATTACKED_THIS_TURN,
                            ..ObjectClause::any()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "~ dealt damage to it this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::Clause(ObjectClause {
                            flags: ObjectFilterFlags::DEALT_DAMAGE_BY_SELF_THIS_TURN,
                            ..ObjectClause::any()
                        })),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "that creature was dealt excess damage this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::DamageReceivedThisTurn {
                        subject: ValueObjectRef::TriggerSubject,
                        excess_only: true,
                    }),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "it's the second creature spell you cast this turn" => Some(TriggerCondition::And(
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: MatchObjectRef::TriggerSubject,
                        filter: Box::new(ObjectFilter::creature()),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }))),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::SpellsCastMatchingThisTurn {
                        players: AffectedPlayers::One(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::creature()),
                        from_zone: None,
                    }),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(2),
                }))),
            )),
            "a player discarded a card this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Or(
                    Box::new(TriggerCondition::ControllerDiscardedThisTurn),
                    Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::Discarded,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    }))),
                ))
            }
            "a permanent left the battlefield this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Or(
                    Box::new(TriggerCondition::PermanentLeftBattlefieldThisTurn),
                    Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::PermanentLeft,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    }))),
                ))
            }
            "no spells were cast this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Not(Box::new(TriggerCondition::Or(
                    Box::new(TriggerCondition::ControllerCastSpellsThisTurn(1)),
                    Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::CastSpells,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    }))),
                ))))
            }
            "you have more life than an opponent" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: Comparison::Greater,
                    right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                })))
            }
            "you created a token this turn"
            | "you created one or more tokens this turn"
            | "you've created a token this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CreatedTokensThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a creature you control died this turn"
            | "a creature you controlled died this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControlledCreatureDiedThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a creature died this turn" => Some(TriggerCondition::Morbid),
            "a land entered the battlefield under your control this turn"
            | "a land entered under your control this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::LandsEnteredThisTurn(
                        ResolvingPlayer::Controller,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a creature entered the battlefield under your control this turn"
            | "a creature entered under your control this turn" => {
                Some(TriggerCondition::CreatureEnteredUnderControlThisTurn)
            }
            "you played a land this turn" | "you've played a land this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you've cast a spell this turn" | "you cast a spell this turn" => {
                Some(TriggerCondition::ControllerCastSpellsThisTurn(1))
            }
            "you haven't cast a spell this turn" | "you didn't cast a spell this turn" => Some(
                TriggerCondition::Not(Box::new(TriggerCondition::ControllerCastSpellsThisTurn(1))),
            ),
            "you've gained life this turn" => Some(TriggerCondition::ControllerGainedLifeThisTurn),
            "you've lost life this turn" => Some(TriggerCondition::ControllerLostLifeThisTurn),
            "you've discarded a card this turn" => {
                Some(TriggerCondition::ControllerDiscardedThisTurn)
            }
            "you've sacrificed a permanent this turn" => {
                Some(TriggerCondition::ControllerSacrificedThisTurn)
            }
            "you've attacked this turn" => Some(TriggerCondition::ControllerAttackedThisTurn),
            "a card left your graveyard this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a card was put into your graveyard from anywhere this turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CardsEnteredGraveyardThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "you control no other creatures" => Some(TriggerCondition::NoOtherControlledCreatures),
            "~ is saddled" => Some(TriggerCondition::SourceSaddled),
            "~ is prepared" => Some(TriggerCondition::SourceIsPrepared),
            "~ didn't attack this turn" => Some(TriggerCondition::SourceDidNotAttackThisTurn),
            "~ is untapped" => Some(TriggerCondition::SourceIsUntapped),
            "~ is tapped" => Some(TriggerCondition::SourceIsTapped),
            "~ is attacking" => Some(TriggerCondition::SourceIsAttacking),
            "you control a land of each basic land type" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::DomainCount,
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(5),
                })))
            }
            "you control a creature of each color" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::DistinctColors {
                        filter: Box::new(ObjectFilter::creature().controlled()),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(5),
                })))
            }
            "it's the first combat phase of the turn" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CombatPhasesThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent gained life this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::GainedLife,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent lost life this turn" => Some(TriggerCondition::OpponentLostLifeAtLeast(1)),
            "an opponent discarded a card this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::Discarded,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent sacrificed a permanent this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::Sacrificed,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent cast a spell this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::CastSpells,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent drew a card this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::DrewCards,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent attacked this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::Attacked,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "an opponent played a land this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::PlayedLand,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a permanent an opponent controlled left the battlefield this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::PermanentLeft,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "a creature entered under an opponent's control this turn"
            | "a creature entered the battlefield under an opponent's control this turn" => {
                use spellwake_engine::program::OpponentTurnStat;
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                        OpponentTurnStat::CreatureEntered,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            "~ was cast using web-slinging" | "they were cast using web-slinging" => {
                Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::SourceState(SourceStateExpr::PaidAlternative),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                })))
            }
            _ => None,
        },
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            action,
        } => {
            use spellwake_engine::program::OpponentTurnStat;
            let act_low = action.trim().trim_end_matches('.').to_ascii_lowercase();
            let stat = match act_low.as_str() {
                "discarded_card"
                | "discarded"
                | "discard"
                | "an opponent discarded a card this turn" => Some(OpponentTurnStat::Discarded),
                "drew_card" | "drew" | "an opponent drew a card this turn" => {
                    Some(OpponentTurnStat::DrewCards)
                }
                "gained_life" | "gain_life" | "an opponent gained life this turn" => {
                    Some(OpponentTurnStat::GainedLife)
                }
                "lost_life" | "lose_life" | "an opponent lost life this turn" => {
                    Some(OpponentTurnStat::LostLife)
                }
                "sacrificed_permanent"
                | "sacrificed"
                | "sacrifice"
                | "an opponent sacrificed a permanent this turn" => {
                    Some(OpponentTurnStat::Sacrificed)
                }
                "attacked" | "attack" | "an opponent attacked this turn" => {
                    Some(OpponentTurnStat::Attacked)
                }
                "cast" | "cast_spell" | "cast_spells" | "an opponent cast a spell this turn" => {
                    Some(OpponentTurnStat::CastSpells)
                }
                "play_land" | "played_land" | "an opponent played a land this turn" => {
                    Some(OpponentTurnStat::PlayedLand)
                }
                "creature_entered"
                | "a creature entered under an opponent's control this turn"
                | "a creature entered the battlefield under an opponent's control this turn" => {
                    Some(OpponentTurnStat::CreatureEntered)
                }
                "permanent_left"
                | "permanent_left_battlefield"
                | "revolt"
                | "a permanent an opponent controlled left the battlefield this turn" => {
                    Some(OpponentTurnStat::PermanentLeft)
                }
                _ => None,
            }?;
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(stat)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer,
            action,
        } => {
            use spellwake_engine::program::OpponentTurnStat;
            let act_low = action.trim().trim_end_matches('.').to_ascii_lowercase();
            let (ctrl_cond, opp_stat) = match act_low.as_str() {
                "discarded_card"
                | "discarded"
                | "discard"
                | "a player discarded a card this turn" => (
                    TriggerCondition::ControllerDiscardedThisTurn,
                    OpponentTurnStat::Discarded,
                ),
                "gained_life" | "gain_life" | "a player gained life this turn" => (
                    TriggerCondition::ControllerGainedLifeThisTurn,
                    OpponentTurnStat::GainedLife,
                ),
                "lost_life" | "lose_life" | "a player lost life this turn" => (
                    TriggerCondition::ControllerLostLifeThisTurn,
                    OpponentTurnStat::LostLife,
                ),
                "sacrificed_permanent"
                | "sacrificed"
                | "sacrifice"
                | "a player sacrificed a permanent this turn" => (
                    TriggerCondition::ControllerSacrificedThisTurn,
                    OpponentTurnStat::Sacrificed,
                ),
                "attacked" | "attack" | "a player attacked this turn" => (
                    TriggerCondition::ControllerAttackedThisTurn,
                    OpponentTurnStat::Attacked,
                ),
                "drew_card" | "drew" | "a player drew a card this turn" => (
                    TriggerCondition::ControllerDrewCardsThisTurn(1),
                    OpponentTurnStat::DrewCards,
                ),
                "cast" | "cast_spell" | "cast_spells" | "a player cast a spell this turn" => (
                    TriggerCondition::ControllerCastSpellsThisTurn(1),
                    OpponentTurnStat::CastSpells,
                ),
                "play_land" | "played_land" | "a player played a land this turn" => (
                    TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })),
                    OpponentTurnStat::PlayedLand,
                ),
                "creature_entered"
                | "a creature entered this turn"
                | "a creature entered the battlefield this turn" => (
                    TriggerCondition::CreatureEnteredUnderControlThisTurn,
                    OpponentTurnStat::CreatureEntered,
                ),
                "permanent_left"
                | "permanent_left_battlefield"
                | "a permanent left the battlefield this turn" => (
                    TriggerCondition::PermanentLeftBattlefieldThisTurn,
                    OpponentTurnStat::PermanentLeft,
                ),
                _ => return None,
            };
            Some(TriggerCondition::Or(
                Box::new(ctrl_cond),
                Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(opp_stat)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                }))),
            ))
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a)
                        if is_self_anaphora_label(a)
                            || a.trim().eq_ignore_ascii_case("it")
                ) =>
        {
            let is_not_owned_only = (filter.owner == Some(ControllerRelation::OwnerOpponent)
                || filter.controller == Some(ControllerRelation::OwnerOpponent)
                || filter.predicates.as_slice()
                    == [spellwake_oracle_compiler::ast::FilterPredicateAst::NotOwnedByController])
                && filter.any_types.is_empty()
                && filter.forbidden_types.is_empty()
                && filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.subtypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.zone.is_none()
                && filter.extra_zones.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.counter_kind.is_none()
                && filter.named.is_none()
                && filter.flags.is_empty();
            if is_not_owned_only {
                return Some(TriggerCondition::SourceNotOwnedByController);
            }
            if !matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && !matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a) if is_self_anaphora_label(a)
                )
            {
                return None;
            }
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            Some(TriggerCondition::SourceMatches(Box::new(obj)))
        }
        ConditionAst::MatchesFilter { subject, filter }
            if {
                let inner = match subject.as_ref() {
                    SubjectRefAst::InZone {
                        subject: inner,
                        zone: ZoneFilter::Battlefield,
                    } => inner.as_ref(),
                    other => other,
                };
                matches!(
                    inner,
                    SubjectRefAst::AttachedHost(_) | SubjectRefAst::TriggerSubject
                ) || matches!(
                    inner,
                    SubjectRefAst::Anaphora(a)
                        if matches!(
                            a.trim().to_ascii_lowercase().as_str(),
                            "that creature"
                                | "that permanent"
                                | "that artifact"
                                | "that enchantment"
                                | "that land"
                                | "that card"
                                | "that spell"
                                | "that token"
                                | "that planeswalker"
                                | "that battle"
                                | "that equipment"
                                | "that aura"
                                | "that vehicle"
                                | "that mount"
                                | "that saga"
                        )
                )
            } =>
        {
            let inner = match subject.as_ref() {
                SubjectRefAst::InZone {
                    subject: inner,
                    zone: ZoneFilter::Battlefield,
                } => inner.as_ref(),
                other => other,
            };
            let match_ref = if matches!(inner, SubjectRefAst::AttachedHost(_)) {
                MatchObjectRef::AttachedHost
            } else {
                MatchObjectRef::TriggerSubject
            };
            let obj = lower_object_filter(filter, vocabulary).ok()?;
            let rc = ResolutionCondition {
                left: ValueExpr::ObjectMatches {
                    subject: match_ref,
                    filter: Box::new(obj),
                },
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            };
            rc.is_trigger_world_evaluable()
                .then(|| TriggerCondition::Eval(Box::new(rc)))
        }
        ConditionAst::HasKeyword { subject, keyword }
            if matches!(
                subject.as_ref(),
                SubjectRefAst::SelfObject | SubjectRefAst::AttachedHost(_)
            ) || matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a)
                    if is_self_anaphora_label(a)
                        || matches!(
                            a.trim().to_ascii_lowercase().as_str(),
                            "it" | "that creature"
                                | "that permanent"
                                | "that artifact"
                                | "that enchantment"
                                | "that land"
                                | "that card"
                                | "that spell"
                                | "that token"
                                | "that planeswalker"
                                | "that battle"
                                | "that equipment"
                                | "that aura"
                                | "that vehicle"
                                | "that mount"
                                | "that saga"
                        )
            ) =>
        {
            let kw_low = keyword.trim().to_ascii_lowercase();
            let obj = if kw_low == "toxic" {
                ObjectFilter::Clause(ObjectClause {
                    flags: ObjectFilterFlags::HAS_TOXIC,
                    ..ObjectClause::any()
                })
            } else {
                let kw = lower_keyword_ability_name(keyword)?;
                ObjectFilter::any().with_keyword(kw)
            };
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(a) if is_self_anaphora_label(a)
                )
            {
                Some(TriggerCondition::SourceMatches(Box::new(obj)))
            } else {
                let match_ref = if matches!(subject.as_ref(), SubjectRefAst::AttachedHost(_)) {
                    MatchObjectRef::AttachedHost
                } else {
                    MatchObjectRef::TriggerSubject
                };
                let rc = ResolutionCondition {
                    left: ValueExpr::ObjectMatches {
                        subject: match_ref,
                        filter: Box::new(obj),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                };
                rc.is_trigger_world_evaluable()
                    .then(|| TriggerCondition::Eval(Box::new(rc)))
            }
        }
        ConditionAst::Marker(GameMarker::RemainsOnBattlefield) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(
                    spellwake_engine::program::SourceZoneKind::Battlefield,
                )),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::EventOccurredThisTurn { event } => {
            match event
                .trim()
                .trim_end_matches('.')
                .to_ascii_lowercase()
                .as_str()
            {
                "you didn't activate a loyalty ability of a planeswalker this turn" => {
                    Some(TriggerCondition::NoLoyaltyActivationThisTurn)
                }
                _ => None,
            }
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
        ConditionAst::Marker(GameMarker::NonlandPermanentLeftThisTurn) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(
                    PlayerStatExpr::AnyNonlandPermanentLeftBattlefieldThisTurn,
                ),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::Marker(GameMarker::SpellWarpedThisTurn) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::AnySpellWarpedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::Marker(GameMarker::YouAreAttacking) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsAttacking),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::Marker(GameMarker::YouAreDefending) => {
            Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsDefending),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })))
        }
        ConditionAst::Compare { lhs, op, rhs }
            if matches!(
                (lhs.as_ref(), rhs.as_ref()),
                (
                    ValueExprAst::PlayerStat {
                        player: PlayerRefAst::You,
                        stat: PlayerStatKind::LifeTotal,
                    },
                    ValueExprAst::PlayerStat {
                        player: PlayerRefAst::AnyOpponent,
                        stat: PlayerStatKind::LifeTotal,
                    },
                )
            ) =>
        {
            match op {
                CompareOp::Gt => Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: Comparison::Greater,
                    right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                }))),
                CompareOp::Ge => Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife),
                }))),
                CompareOp::Lt => Some(TriggerCondition::OpponentHasMoreLife),
                CompareOp::Le => Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                    comparison: Comparison::LessOrEqual,
                    right: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                }))),
                _ => None,
            }
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
        ConditionAst::ZoneChangeThisTurn {
            filter,
            from_zone,
            to_zone,
            controller,
            min_count,
        } if *min_count > 0 => {
            use spellwake_engine::program::OpponentTurnStat;
            let is_plain_extras = filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.subtypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.owner.is_none()
                && filter.extra_zones.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.stat_bounds.is_empty()
                && filter.counter_kind.is_none()
                && filter.named.is_none()
                && (filter.flags.0 & !AstFilterFlags::LKI_PAST_TENSE.0) == 0
                && filter.predicates.is_empty();
            if *from_zone == Some(ZoneFilter::Battlefield)
                && matches!(
                    to_zone,
                    Some(
                        ZoneFilter::AnyGraveyard
                            | ZoneFilter::AllGraveyards
                            | ZoneFilter::SingleGraveyard
                    )
                )
                && filter.owner.is_none()
                && filter.extra_zones.is_empty()
                && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            {
                let (ctrl, eff_filter_ctrl) = match controller {
                    None => (None, filter.controller),
                    Some(PlayerRefAst::You)
                        if matches!(
                            filter.controller,
                            None | Some(ControllerRelation::You | ControllerRelation::YourTeam)
                        ) =>
                    {
                        (Some(ResolvingPlayer::Controller), filter.controller)
                    }
                    Some(PlayerRefAst::AnyOpponent)
                        if *min_count == 1
                            && matches!(
                                filter.controller,
                                None | Some(ControllerRelation::Opponent)
                            ) =>
                    {
                        (None, Some(ControllerRelation::Opponent))
                    }
                    _ => return None,
                };
                let mut clean = filter.as_ref().clone();
                clean.zone = None;
                clean.controller = eff_filter_ctrl;
                let obj = lower_object_filter(&clean, vocabulary).ok()?;
                if !filter_supported_by_zone_move_record(&obj) {
                    return None;
                }
                if ctrl.is_none() && *min_count == 1 && obj == ObjectFilter::creature() {
                    return Some(TriggerCondition::Morbid);
                }
                let rc = ResolutionCondition {
                    left: ValueExpr::PlayerStat(
                        PlayerStatExpr::GraveyardArrivalsMatchingThisTurn {
                            filter: Box::new(obj),
                            from_battlefield_only: true,
                            controller: ctrl,
                        },
                    ),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                };
                return rc
                    .is_trigger_world_evaluable()
                    .then(|| TriggerCondition::Eval(Box::new(rc)));
            }
            if *from_zone == Some(ZoneFilter::Battlefield)
                && to_zone.is_none()
                && is_plain_extras
                && matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
            {
                let your_ctrl = (matches!(controller, Some(PlayerRefAst::You))
                    && matches!(
                        filter.controller,
                        None | Some(ControllerRelation::You | ControllerRelation::YourTeam)
                    ))
                    || (controller.is_none()
                        && matches!(
                            filter.controller,
                            Some(ControllerRelation::You | ControllerRelation::YourTeam)
                        ));
                let opp_ctrl = (matches!(controller, Some(PlayerRefAst::AnyOpponent))
                    && matches!(filter.controller, None | Some(ControllerRelation::Opponent)))
                    || (controller.is_none()
                        && filter.controller == Some(ControllerRelation::Opponent));
                let any_ctrl = controller.is_none() && filter.controller.is_none();
                if your_ctrl && filter.any_types.is_empty() && filter.forbidden_types.is_empty() {
                    if *min_count == 1 {
                        return Some(TriggerCondition::PermanentLeftBattlefieldThisTurn);
                    }
                    return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::PermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    })));
                }
                if your_ctrl
                    && filter.any_types.is_empty()
                    && filter.forbidden_types == AstCardTypeMask::LAND
                {
                    return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    })));
                }
                if opp_ctrl
                    && *min_count == 1
                    && filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                {
                    return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::PermanentLeft,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })));
                }
                if any_ctrl
                    && filter.any_types.is_empty()
                    && filter.forbidden_types == AstCardTypeMask::LAND
                {
                    return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(
                            PlayerStatExpr::AnyNonlandPermanentLeftBattlefieldThisTurn,
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(i64::from(*min_count)),
                    })));
                }
                if any_ctrl
                    && *min_count == 1
                    && filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                {
                    return Some(TriggerCondition::Or(
                        Box::new(TriggerCondition::PermanentLeftBattlefieldThisTurn),
                        Box::new(TriggerCondition::Eval(Box::new(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                                OpponentTurnStat::PermanentLeft,
                            )),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        }))),
                    ));
                }
            }
            if from_zone.is_none()
                && *to_zone == Some(ZoneFilter::Battlefield)
                && is_plain_extras
                && filter.zone.is_none()
                && filter.forbidden_types.is_empty()
            {
                if matches!(controller, Some(PlayerRefAst::AnyOpponent))
                    && *min_count == 1
                    && filter.any_types == AstCardTypeMask::CREATURE
                    && matches!(filter.controller, None | Some(ControllerRelation::Opponent))
                {
                    return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(
                            OpponentTurnStat::CreatureEntered,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })));
                }
                if matches!(controller, Some(PlayerRefAst::You))
                    && matches!(
                        filter.controller,
                        None | Some(ControllerRelation::You | ControllerRelation::YourTeam)
                    )
                {
                    if filter.any_types.is_empty() {
                        return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredPermanentThisTurn),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(i64::from(*min_count)),
                        })));
                    }
                    if filter.any_types == AstCardTypeMask::CREATURE {
                        if *min_count == 1 {
                            return Some(TriggerCondition::CreatureEnteredUnderControlThisTurn);
                        }
                        return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                            left: ValueExpr::PlayerStat(
                                PlayerStatExpr::CreatureEnteredUnderControlThisTurn,
                            ),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(i64::from(*min_count)),
                        })));
                    }
                    if let Ok(mut types) = card_type_mask_to_set(filter.any_types)
                        && types.len() == 1
                        && let Some(kind) = types.pop_first()
                    {
                        return Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                            left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredTypeThisTurn(kind)),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(i64::from(*min_count)),
                        })));
                    }
                }
            }
            None
        }
        ConditionAst::GameStateFlag(flag) => {
            let fl = flag.trim().trim_end_matches('.').to_ascii_lowercase();
            match fl.as_str() {
                "day" | "it's day" | "it is day" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(1),
                    })))
                }
                "night" | "it's night" | "it is night" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(2),
                    })))
                }
                "neither day nor night"
                | "it's neither day nor night"
                | "it is neither day nor night" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    })))
                }
                "city's blessing" | "you have the city's blessing" | "citys_blessing" => {
                    Some(TriggerCondition::ControllerHasCityBlessing)
                }
                "monarch" | "you're the monarch" | "you are the monarch" => {
                    Some(TriggerCondition::Monarch { opponent: false })
                }
                "an opponent is the monarch" | "opponent is the monarch" => {
                    Some(TriggerCondition::Monarch { opponent: true })
                }
                "there is no monarch" | "no one is the monarch" | "no monarch" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::NoMonarch),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })))
                }
                "initiative" | "you have the initiative" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::HasInitiative),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })))
                }
                "max speed"
                | "you have max speed"
                | "your speed is 4"
                | "your speed is 4 or greater"
                | "speed >= 4" => Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(4),
                }))),
                "your speed is 1 or greater" | "speed >= 1" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    })))
                }
                "your speed is 2 or greater" | "speed >= 2" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(2),
                    })))
                }
                "your speed is 3 or greater" | "speed >= 3" => {
                    Some(TriggerCondition::Eval(Box::new(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(3),
                    })))
                }
                "you don't own it"
                | "you don't own ~"
                | "you don't own this creature"
                | "you don't own this permanent"
                | "it isn't owned by you"
                | "it is not owned by you"
                | "~ isn't owned by you"
                | "~ is not owned by you" => Some(TriggerCondition::SourceNotOwnedByController),
                _ => None,
            }
        }
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
        ConditionAst::GameStateFlag(_) => Err("oracle:unsupported-trigger-condition"),
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

#[cfg(test)]
mod wave10_turn_condition_tests {
    use super::*;

    #[test]
    fn glademuse_and_price_of_glory_lower_complete_off_turn_triggers() {
        let vocab = CreatureTypeVocabulary::new("test-vocab".into(), [0; 32], vec!["Beast".into()])
            .unwrap();
        for (name, type_line, mana_cost, pt, oracle_text) in [
            (
                "Glademuse",
                "Creature \u{2014} Beast",
                "{2}{G}",
                Some(("2", "4")),
                "Whenever a player casts a spell, if it's not their turn, that player draws a card.",
            ),
            (
                "Price of Glory",
                "Enchantment",
                "{2}{R}",
                None,
                "Whenever a player taps a land for mana, if it's not that player's turn, destroy that land.",
            ),
        ] {
            let card = InventoryCard {
                scryfall_id: "00000000-0000-0000-0000-000000000000".into(),
                oracle_id: Some("00000000-0000-0000-0000-000000000001".into()),
                name: name.into(),
                layout: "normal".into(),
                faces: vec![spellwake_content::CatalogFace {
                    name: name.into(),
                    type_line: Some(type_line.into()),
                    oracle_text: Some(oracle_text.into()),
                    mana_cost: Some(mana_cost.into()),
                    power: pt.map(|(power, _)| power.into()),
                    toughness: pt.map(|(_, toughness)| toughness.into()),
                }],
                legalities: BTreeMap::new(),
                eligible_formats: vec![],
                required_by_base_formats: true,
                program_evidence: "test",
            };
            let program = compile_with_reason(&card, &vocab)
                .unwrap_or_else(|reason| panic!("{name}: {reason}"));
            assert_eq!(program.triggered.len(), 1);
            assert!(matches!(
                program.triggered[0].condition.intervening_if,
                Some(TriggerCondition::Not(_))
            ));
        }
    }

    #[test]
    fn related_player_turn_condition_preserves_negation_and_role() {
        let vocab = CreatureTypeVocabulary::new("test-vocab".into(), [0; 32], vec!["Human".into()])
            .unwrap();
        let turn = ConditionAst::IsPlayersTurn(PlayerRefAst::ThatPlayer);
        let expected = TriggerCondition::Eval(Box::new(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerIsActivePlayer(
                ResolvingPlayer::UniqueRelatedPlayer,
            )),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }));
        assert_eq!(lower_trigger_condition(&turn, &vocab), Ok(expected.clone()));
        assert_eq!(
            lower_trigger_condition(&ConditionAst::Not(Box::new(turn.clone())), &vocab),
            Ok(TriggerCondition::Not(Box::new(expected)))
        );
        assert!(
            lower_resolution_condition(&turn, &mut BodyBuilder::new(), &vocab, false, None)
                .is_err()
        );
    }
}
