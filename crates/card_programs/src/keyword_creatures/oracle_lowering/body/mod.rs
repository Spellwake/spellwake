use super::*;
pub(super) use control_flow::try_wrap_each_player_that_player;

// =========================================================================
// Effect Body & Instruction Lowering
// =========================================================================

mod animate_offers;
mod conditions;
mod conditions_markers;
mod control_flow;
mod restrictions;

use self::animate_offers::*;
use self::conditions::*;
use self::conditions_markers::*;
use self::control_flow::*;
use self::restrictions::*;

pub(super) use self::animate_offers::lower_animate_or_set_pt;
pub(super) use self::conditions::{
    is_self_pronoun, lower_comparison, lower_resolution_condition, negate_comparison,
    trigger_subject_is_self,
};
pub(super) use self::control_flow::lower_instruction_seq;
pub(super) use self::restrictions::{
    apply_trigger_body_rewrites, body_references_self_from_graveyard,
    body_returns_self_from_graveyard, combine_activation_restrictions, extract_modal_with_wrap,
    lower_conditional_activation_restriction, parse_activation_restriction,
    parse_cast_timing_restriction, parse_modal_choice, parse_modal_choice_with_uniqueness,
    rewrite_body_anaphora_stats, rewrite_body_prior_result, rewrite_instruction_anaphora,
    rewrite_value_expr_anaphora, rewrite_value_expr_prior_result, strip_activated_restrictions,
    strip_triggered_restrictions, try_lower_exact_plus_upto_activated_modes,
    try_lower_exact_plus_upto_triggered_modes, try_lower_independent_multi_clause_activated_modes,
    try_lower_independent_multi_clause_triggered_modes,
};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TriggerHint {
    #[default]
    None,
    CombatBlock,
    DealtDamage,
}

impl TriggerHint {
    pub(super) fn from_event(event: &TriggerEvent) -> Self {
        match event {
            TriggerEvent::BlocksOrBlocked { .. } => Self::CombatBlock,
            TriggerEvent::DealtDamage { .. }
            | TriggerEvent::DealsDamage { .. }
            | TriggerEvent::DealsNoncombatDamage { .. } => Self::DealtDamage,
            _ => Self::None,
        }
    }
}

#[derive(Clone)]
pub(super) struct BodyBuilder {
    pub(super) target: BodyTarget,
    pub(super) instructions: Vec<Instruction>,
    pub(super) graveyard_target_bounds: Option<(usize, usize)>,
    pub(super) permanent_target_bounds: Option<(u16, u16)>,
    pub(super) x_target_count: bool,
    pub(super) up_to_x_target_count: bool,
    pub(super) up_to_one_target: bool,
    pub(super) last_group_filter: Option<ObjectFilter>,
    pub(super) trigger_hint: TriggerHint,
    pub(super) has_source: bool,
}

impl BodyBuilder {
    pub(super) fn new() -> Self {
        Self {
            target: BodyTarget::None,
            instructions: Vec::new(),
            graveyard_target_bounds: None,
            permanent_target_bounds: None,
            x_target_count: false,
            up_to_x_target_count: false,
            up_to_one_target: false,
            last_group_filter: None,
            trigger_hint: TriggerHint::None,
            has_source: false,
        }
    }

    pub(super) fn with_trigger_hint(hint: TriggerHint) -> Self {
        Self {
            trigger_hint: hint,
            ..Self::new()
        }
    }

    pub(super) fn bind_target(&mut self, next: BodyTarget) -> Result<(), &'static str> {
        if next == BodyTarget::None {
            return Ok(());
        }
        if self
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
        {
            return Err("oracle:multiple-targets");
        }
        if self.target == BodyTarget::None {
            self.target = next;
            Ok(())
        } else if self.target == next {
            // Multiple instructions referencing the same single target are only safe
            // when the second instruction uses anaphora; reject distinct target clauses.
            Err("oracle:multiple-targets")
        } else {
            Err("oracle:conflicting-targets")
        }
    }

    pub(super) fn push_bounded_target_op(&mut self, minimum: u16, maximum: u16, op: Instruction) {
        self.permanent_target_bounds = Some((minimum, maximum));
        self.instructions.push(op);
    }

    pub(super) fn finish(self) -> Result<ExecutableBody, &'static str> {
        if self.instructions.is_empty() {
            return Err("oracle:empty-body");
        }
        if self.target != BodyTarget::None
            && self
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
        {
            return Err("oracle:conflicting-targets");
        }
        let mut body = ExecutableBody::new(self.target, self.instructions);
        if body.target == BodyTarget::None {
            rewrite_body_after_revealed_top(&mut body);
        }
        rewrite_body_after_amass(&mut body);
        let mut body = match self.graveyard_target_bounds {
            Some((min, max)) => body.with_graveyard_target_bounds(min, max),
            None => body,
        };
        if let Some((min, max)) = self.permanent_target_bounds {
            body = body.with_permanent_target_bounds(min, max);
        }
        if self.x_target_count {
            body = body.with_x_target_count();
        }
        if self.up_to_x_target_count {
            body = body.with_up_to_x_target_count();
        }
        if self.up_to_one_target {
            body = body.with_up_to_one_target();
        }
        Ok(body)
    }
}

fn rewrite_body_after_revealed_top(body: &mut ExecutableBody) {
    let mut saw_reveal_top = false;
    for op in &mut body.instructions {
        if saw_reveal_top {
            rewrite_instruction_anaphora(op, ValueObjectRef::RevealedTop);
        }
        if matches!(
            op,
            Instruction::LookTopChooseExt {
                reveal_only: true,
                ..
            }
        ) || matches!(
            op,
            Instruction::MutatePlayer(spec)
                if matches!(
                    spec.op,
                    spellwake_engine::toolbox::PlayerMutation::MayRevealLibraryTop
                )
        ) || matches!(
            op,
            Instruction::ZoneOp(spec)
                if matches!(
                    spec.source,
                    spellwake_engine::toolbox::ObjectQuery::ZoneScan {
                        zone: spellwake_engine::toolbox::QueryZone::Library(_),
                        slice: spellwake_engine::toolbox::QuerySlice::Top(_),
                        ..
                    }
                ) && matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Exile { .. }
                )
        ) {
            saw_reveal_top = true;
        }
    }
}

fn rewrite_body_after_amass(body: &mut ExecutableBody) {
    if is_object_body_target(body.target, false) {
        return;
    }
    let mut saw_amass = false;
    for op in &mut body.instructions {
        if saw_amass {
            rewrite_instruction_anaphora(op, ValueObjectRef::LastAmassedArmy);
        }
        if matches!(
            op,
            Instruction::MutatePlayer(spec)
                if matches!(spec.op, spellwake_engine::toolbox::PlayerMutation::Amass { .. })
        ) {
            saw_amass = true;
        }
    }
}

/// Insert an instruction at `pos` while relocating every jump destination `>= pos + 1`
/// across the entire instruction slice so forward/branch/offer/coin offsets remain exact.
pub(super) fn prepend_instruction_at(
    instructions: &mut Vec<Instruction>,
    pos: usize,
    inst: Instruction,
) {
    for op in instructions.iter_mut() {
        op.shift_jumps(1, pos + 1);
    }
    instructions.insert(pos, inst);
}

pub(super) fn lower_spell_effects(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(ExecutableBody, Option<SpellModes>), &'static str> {
    // Spree spells: one or more SpreeMode { cost, body } instructions (with optional prefix/suffix).
    let spree_indices: Vec<usize> = ins
        .iter()
        .enumerate()
        .filter_map(|(idx, op)| matches!(op, InstructionAst::SpreeMode { .. }).then_some(idx))
        .collect();
    if spree_indices.len() >= 2 && spree_indices.windows(2).all(|pair| pair[1] == pair[0] + 1) {
        let first_spree = spree_indices[0];
        let last_spree = *spree_indices.last().unwrap_or(&first_spree);
        let (pre_clean, pre_distinct) = strip_each_mode_different_player(&ins[..first_spree]);
        let (post_clean, post_distinct) = strip_each_mode_different_player(&ins[last_spree + 1..]);
        let mut distinct_targets = pre_distinct || post_distinct;
        let prefix = if pre_clean.is_empty() {
            None
        } else {
            let pb = lower_effect_body_impl(&pre_clean, vocabulary, ids, false, false, false)?;
            if pb.target != BodyTarget::None {
                return Err("oracle:modal-prefix-target");
            }
            Some(Box::new(pb))
        };
        let suffix = if post_clean.is_empty() {
            None
        } else {
            let sb = lower_effect_body_impl(&post_clean, vocabulary, ids, false, false, false)?;
            if sb.target != BodyTarget::None {
                return Err("oracle:modal-suffix-target");
            }
            Some(Box::new(sb))
        };
        let mut lowered = Vec::with_capacity(spree_indices.len());
        let mut mode_costs = Vec::with_capacity(spree_indices.len());
        for &idx in &spree_indices {
            let InstructionAst::SpreeMode { cost, body } = &ins[idx] else {
                continue;
            };
            let mut mode_mana = ManaCost::free();
            for atom in cost {
                if let CostAtomAst::PayMana(raw) = atom
                    && let Some(mc) = parse_fixed_mana_cost(raw)
                {
                    mode_mana.generic = mode_mana.generic.plus(&mc.generic);
                    for (&color, amt) in &mc.required {
                        let slot = mode_mana.required.entry(color).or_insert_with(|| 0.into());
                        *slot = slot.plus(amt);
                    }
                } else {
                    return Err("oracle:spree-non-mana-cost");
                }
            }
            mode_costs.push(mode_mana);
            let (mode_clean, mode_distinct) = strip_each_mode_different_player(body);
            distinct_targets |= mode_distinct;
            let mode_body =
                lower_effect_body_impl(&mode_clean, vocabulary, ids, false, false, true)?;
            lowered.push(mode_body);
        }
        let max = u8::try_from(lowered.len()).unwrap_or(u8::MAX);
        let first = lowered.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: 1,
                maximum: max,
                conditional_maximum: None,
                allow_repeat: false,
                distinct_targets,
                uniqueness: ModalUniqueness::None,
                mode_costs,
                escalate_mana: None,
                prefix,
                suffix,
                additional: lowered,
            }),
        ));
    }
    let normalized_split_modal = normalize_split_conditional_modal(ins);
    let modal_ins = normalized_split_modal.as_deref().unwrap_or(ins);
    if let Some((pre, choice, modes, post, allow_repeat)) = extract_modal_with_wrap(modal_ins) {
        if modes.len() < 2 {
            return Err("oracle:modal-too-few");
        }
        let modes_len = u8::try_from(modes.len()).unwrap_or(u8::MAX);
        let ParsedModalChoice {
            min,
            raw_max,
            uniqueness,
            mut conditional_maximum,
        } = parse_modal_choice_with_conditional_max(choice, modes_len, vocabulary)?;
        let max = if allow_repeat {
            raw_max
        } else {
            raw_max.min(modes_len)
        };
        let (pre_nodist, pre_distinct) = strip_each_mode_different_player(pre);
        let (post_nodist, post_distinct) = strip_each_mode_different_player(post);
        let (pre_clean, pre_cond_max) =
            strip_conditional_modal_max(&pre_nodist, modes_len, vocabulary)?;
        let (post_clean, post_cond_max) =
            strip_conditional_modal_max(&post_nodist, modes_len, vocabulary)?;
        for extra_cm in [pre_cond_max, post_cond_max].into_iter().flatten() {
            if conditional_maximum.is_some() {
                return Err("oracle:multiple-conditional-modal-max");
            }
            conditional_maximum = Some(extra_cm);
        }
        let conditional_maximum = match conditional_maximum {
            Some((cond, raw_cmax)) => {
                let eff_cmax = if allow_repeat {
                    raw_cmax
                } else {
                    raw_cmax.min(modes_len)
                };
                if eff_cmax < min || (!allow_repeat && usize::from(eff_cmax) > modes.len()) {
                    return Err("oracle:unsupported-conditional-modal-max");
                }
                Some((Box::new(cond), eff_cmax))
            }
            None => None,
        };
        let mut stripped_distinct = pre_distinct || post_distinct;
        let (prefix, suffix, embed_wrap) = if pre_clean.is_empty() && post_clean.is_empty() {
            (None, None, false)
        } else if (min, max) == (1, 1) && !allow_repeat && conditional_maximum.is_none() {
            (None, None, true)
        } else {
            let pb = if pre_clean.is_empty() {
                None
            } else {
                let b = lower_effect_body_impl(&pre_clean, vocabulary, ids, false, false, false)?;
                if b.target != BodyTarget::None {
                    return Err("oracle:modal-prefix-target");
                }
                Some(Box::new(b))
            };
            let sb = if post_clean.is_empty() {
                None
            } else {
                let b = lower_effect_body_impl(&post_clean, vocabulary, ids, false, false, false)?;
                if b.target != BodyTarget::None {
                    return Err("oracle:modal-suffix-target");
                }
                Some(Box::new(b))
            };
            (pb, sb, false)
        };
        let all_spree = modes
            .iter()
            .all(|m| matches!(m.body.first(), Some(InstructionAst::SpreeMode { .. })));
        let mut mode_costs = Vec::new();
        let mut lowered = Vec::with_capacity(modes.len());
        for m in modes {
            let raw_mode_slice: Vec<InstructionAst> = if all_spree
                && let Some(InstructionAst::SpreeMode { cost, body }) = m.body.first()
            {
                let mut mode_mana = ManaCost::free();
                for atom in cost {
                    if let CostAtomAst::PayMana(raw) = atom
                        && let Some(mc) = parse_fixed_mana_cost(raw)
                    {
                        mode_mana.generic = mode_mana.generic.plus(&mc.generic);
                        for (&color, amt) in &mc.required {
                            let slot = mode_mana.required.entry(color).or_insert_with(|| 0.into());
                            *slot = slot.plus(amt);
                        }
                    } else {
                        return Err("oracle:spree-non-mana-cost");
                    }
                }
                mode_costs.push(mode_mana);
                let mut merged = body.clone();
                merged.extend_from_slice(&m.body[1..]);
                merged
            } else {
                m.body.clone()
            };
            let (mode_body_slice, mode_distinct) =
                strip_each_mode_different_player(&raw_mode_slice);
            stripped_distinct |= mode_distinct;
            let full = if embed_wrap {
                let mut f =
                    Vec::with_capacity(pre_clean.len() + mode_body_slice.len() + post_clean.len());
                f.extend_from_slice(&pre_clean);
                f.extend_from_slice(&mode_body_slice);
                f.extend_from_slice(&post_clean);
                f
            } else {
                mode_body_slice
            };
            let mode_body = lower_effect_body_impl(&full, vocabulary, ids, false, false, true)?;
            lowered.push(mode_body);
        }
        let distinct_targets = {
            let low_c = choice.to_ascii_lowercase();
            stripped_distinct
                || low_c.contains("different player")
                || low_c.contains("different target")
        };
        let first = lowered.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: min,
                maximum: max,
                conditional_maximum,
                allow_repeat,
                distinct_targets,
                uniqueness,
                mode_costs,
                escalate_mana: None,
                prefix,
                suffix,
                additional: lowered,
            }),
        ));
    }
    let mut body = match lower_effect_body_impl(ins, vocabulary, ids, false, false, true) {
        Ok(b) => b,
        Err(
            err @ ("oracle:multiple-targets"
            | "oracle:conflicting-targets"
            | "oracle:invalid-fixed-clause"),
        ) => {
            if let Some((mode0, modes)) =
                try_lower_exact_plus_upto_spell_modes(ins, vocabulary, ids).or_else(|| {
                    try_lower_independent_multi_clause_spell_modes(ins, vocabulary, ids)
                })
            {
                return Ok((mode0, Some(modes)));
            }
            return Err(err);
        }
        Err(err) => return Err(err),
    };
    let saw_sacrifice = body.instructions.iter().any(|op| {
        crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op).is_some_and(
            |spec| {
                matches!(
                    spec.destination,
                    spellwake_engine::toolbox::ZoneDest::Graveyard {
                        semantics: spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                    }
                )
            },
        )
    });
    if saw_sacrifice {
        rewrite_body_anaphora_stats(&mut body, ValueObjectRef::LastSacrificedCreature);
    }
    Ok((body, None))
}

pub(super) fn lower_activated_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<(ExecutableBody, Option<ActivationRestriction>), &'static str> {
    let (filtered, restriction) = strip_activated_restrictions(ins, vocabulary)?;
    let body = lower_effect_body(&filtered, vocabulary, ids, false, true)?;
    Ok((body, restriction))
}

pub(super) fn lower_effect_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<ExecutableBody, &'static str> {
    lower_effect_body_impl(ins, vocabulary, ids, in_trigger, self_pronoun_valid, true)
}

pub(super) fn lower_trigger_effect_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    spec: &TriggerSpec,
    self_pronoun_valid: bool,
) -> Result<ExecutableBody, &'static str> {
    let hint = TriggerHint::from_event(&spec.event);
    let mut builder = BodyBuilder::with_trigger_hint(hint);
    builder.has_source = true;
    match lower_instruction_seq(
        ins,
        &mut builder,
        vocabulary,
        ids,
        true,
        self_pronoun_valid,
        None,
    ) {
        Ok(()) => builder.finish(),
        Err("oracle:multiple-targets" | "oracle:conflicting-targets") => {
            lower_multi_target_fixed_clauses(ins, vocabulary, ids, true, self_pronoun_valid, hint)
        }
        Err(err) => Err(err),
    }
}

pub(super) fn lower_effect_body_impl(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    allow_fixed_clauses: bool,
) -> Result<ExecutableBody, &'static str> {
    lower_effect_body_with_hint(
        ins,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        allow_fixed_clauses,
        TriggerHint::None,
    )
}

pub(super) fn lower_effect_body_with_hint(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    allow_fixed_clauses: bool,
    trigger_hint: TriggerHint,
) -> Result<ExecutableBody, &'static str> {
    let mut builder = BodyBuilder::with_trigger_hint(trigger_hint);
    builder.has_source = in_trigger || self_pronoun_valid;
    match lower_instruction_seq(
        ins,
        &mut builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        None,
    ) {
        Ok(()) => builder.finish(),
        Err("oracle:multiple-targets" | "oracle:conflicting-targets") if allow_fixed_clauses => {
            lower_multi_target_fixed_clauses(
                ins,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                trigger_hint,
            )
        }
        Err(err) => Err(err),
    }
}

pub(super) fn lower_die_roll_band_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    inherited_target: BodyTarget,
) -> Result<ExecutableBody, &'static str> {
    let mut builder = BodyBuilder::new();
    builder.target = inherited_target;
    builder.has_source = in_trigger || self_pronoun_valid;
    lower_instruction_seq(
        ins,
        &mut builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        None,
    )?;
    if builder.target != inherited_target {
        return Err("oracle:die-roll-band-target");
    }
    builder.target = BodyTarget::None;
    builder.finish()
}

pub(super) fn resolve_x<'a>(
    expr: &'a ValueExprAst,
    bound_x: Option<&'a ValueExprAst>,
) -> &'a ValueExprAst {
    if matches!(expr, ValueExprAst::ChosenX)
        && let Some(bx) = bound_x
    {
        return bx;
    }
    expr
}
