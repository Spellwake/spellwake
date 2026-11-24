use super::*;

// =========================================================================
// Effect Body & Instruction Lowering
// =========================================================================

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
    pub(super) x_target_count: bool,
    pub(super) last_group_filter: Option<ObjectFilter>,
    pub(super) trigger_hint: TriggerHint,
}

impl BodyBuilder {
    pub(super) fn new() -> Self {
        Self {
            target: BodyTarget::None,
            instructions: Vec::new(),
            graveyard_target_bounds: None,
            x_target_count: false,
            last_group_filter: None,
            trigger_hint: TriggerHint::None,
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

    pub(super) fn finish(self) -> Result<ExecutableBody, &'static str> {
        if self.instructions.is_empty() {
            return Err("oracle:empty-body");
        }
        let mut body = ExecutableBody::new(self.target, self.instructions);
        if body.target == BodyTarget::None {
            rewrite_body_after_revealed_top(&mut body);
        }
        let mut body = match self.graveyard_target_bounds {
            Some((min, max)) => body.with_graveyard_target_bounds(min, max),
            None => body,
        };
        if self.x_target_count {
            body = body.with_x_target_count();
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
            Instruction::LookTopChoose {
                reveal_only: true,
                ..
            } | Instruction::MayRevealLibraryTop
        ) {
            saw_reveal_top = true;
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

pub(super) type ExtractedModal<'a> = (
    &'a [InstructionAst],
    &'a str,
    &'a [spellwake_oracle_compiler::ast::ModalModeAst],
    &'a [InstructionAst],
    bool,
);

pub(super) fn extract_modal_with_wrap(ins: &[InstructionAst]) -> Option<ExtractedModal<'_>> {
    if let [InstructionAst::Optional(inner)] = ins
        && let Some((pre, choice, modes, post, repeat)) = extract_modal_with_wrap(inner)
        && let Ok((min, max, _)) = parse_modal_choice_with_uniqueness(choice)
    {
        if min == 0 {
            return Some((pre, choice, modes, post, repeat));
        }
        if min == 1 && max == 1 {
            return Some((pre, "choose up to one", modes, post, repeat));
        }
        if min == 1 && max == 2 {
            return Some((pre, "choose up to two", modes, post, repeat));
        }
    }
    let mut modal_idx: Option<usize> = None;
    let mut allow_repeat = false;
    for (idx, item) in ins.iter().enumerate() {
        match item {
            InstructionAst::Modal { .. } => {
                if modal_idx.is_some() {
                    return None;
                }
                modal_idx = Some(idx);
            }
            InstructionAst::AllowRepeatModes => {
                allow_repeat = true;
            }
            _ => {}
        }
    }
    let idx = modal_idx?;
    let InstructionAst::Modal { choice, modes } = &ins[idx] else {
        return None;
    };
    let low_choice = choice.to_ascii_lowercase();
    if low_choice.contains("you may choose the same mode more than once") {
        allow_repeat = true;
    }
    let mut pre = &ins[..idx];
    let mut post = &ins[idx + 1..];
    if matches!(pre.last(), Some(InstructionAst::AllowRepeatModes)) {
        pre = &pre[..pre.len() - 1];
    }
    if matches!(post.first(), Some(InstructionAst::AllowRepeatModes)) {
        post = &post[1..];
    }
    if pre
        .iter()
        .chain(post.iter())
        .any(|op| matches!(op, InstructionAst::AllowRepeatModes))
    {
        return None;
    }
    Some((pre, choice.as_str(), modes.as_slice(), post, allow_repeat))
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
        let pre = &ins[..first_spree];
        let post = &ins[last_spree + 1..];
        let prefix = if pre.is_empty() {
            None
        } else {
            let pb = lower_effect_body_impl(pre, vocabulary, ids, false, false, false)?;
            if pb.target != BodyTarget::None {
                return Err("oracle:modal-prefix-target");
            }
            Some(Box::new(pb))
        };
        let suffix = if post.is_empty() {
            None
        } else {
            let sb = lower_effect_body_impl(post, vocabulary, ids, false, false, false)?;
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
            lowered.push(lower_effect_body_impl(
                body, vocabulary, ids, false, false, false,
            )?);
        }
        let max = u8::try_from(lowered.len()).unwrap_or(u8::MAX);
        let first = lowered.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: 1,
                maximum: max,
                allow_repeat: false,
                distinct_targets: false,
                uniqueness: ModalUniqueness::None,
                mode_costs,
                escalate_mana: None,
                prefix,
                suffix,
                additional: lowered,
            }),
        ));
    }
    if let Some((pre, choice, modes, post, allow_repeat)) = extract_modal_with_wrap(ins) {
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
                let b = lower_effect_body_impl(pre, vocabulary, ids, false, false, false)?;
                if b.target != BodyTarget::None {
                    return Err("oracle:modal-prefix-target");
                }
                Some(Box::new(b))
            };
            let sb = if post.is_empty() {
                None
            } else {
                let b = lower_effect_body_impl(post, vocabulary, ids, false, false, false)?;
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
            let mode_body_slice: Vec<InstructionAst> = if all_spree
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
            let full = if embed_wrap {
                let mut f = Vec::with_capacity(pre.len() + mode_body_slice.len() + post.len());
                f.extend_from_slice(pre);
                f.extend_from_slice(&mode_body_slice);
                f.extend_from_slice(post);
                f
            } else {
                mode_body_slice
            };
            lowered.push(lower_effect_body_impl(
                &full, vocabulary, ids, false, false, false,
            )?);
        }
        let distinct_targets = {
            let low_c = choice.to_ascii_lowercase();
            low_c.contains("different player") || low_c.contains("different target")
        };
        let first = lowered.remove(0);
        return Ok((
            first,
            Some(SpellModes {
                minimum: min,
                maximum: max,
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
    let body = lower_effect_body_impl(ins, vocabulary, ids, false, false, true)?;
    let mut body = body;
    let saw_sacrifice = body.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::Sacrifice { .. } | Instruction::SacrificeMatching(_)
        )
    });
    if saw_sacrifice {
        rewrite_body_anaphora_stats(&mut body, ValueObjectRef::LastSacrificedCreature);
    }
    Ok((body, None))
}

pub(super) fn parse_modal_choice_with_uniqueness(
    choice: &str,
) -> Result<(u8, u8, ModalUniqueness), &'static str> {
    let mut low = choice
        .trim()
        .trim_end_matches('\u{2014}')
        .trim_end_matches('-')
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase();
    for repeat_suffix in [
        ". you may choose the same mode more than once",
        " you may choose the same mode more than once",
        ". each mode must target a different player",
        " each mode must target a different player",
    ] {
        if let Some(rest) = low.strip_suffix(repeat_suffix) {
            low = rest.trim().trim_end_matches('.').trim().to_string();
        }
    }
    let (base, uniqueness) =
        if let Some(rest) = low.strip_suffix(" that hasn't been chosen this turn") {
            (rest.trim(), ModalUniqueness::OncePerTurn)
        } else if let Some(rest) = low.strip_suffix(" that hasn't been chosen") {
            (rest.trim(), ModalUniqueness::OncePerGame)
        } else if let Some(rest) = low.strip_suffix(" that haven't been chosen") {
            (rest.trim(), ModalUniqueness::OncePerGame)
        } else {
            (low.as_str(), ModalUniqueness::None)
        };
    let (min, max) = parse_modal_choice(base)?;
    Ok((min, max, uniqueness))
}

pub(super) fn parse_modal_choice(choice: &str) -> Result<(u8, u8), &'static str> {
    let cleaned = choice
        .trim()
        .trim_end_matches('\u{2014}')
        .trim_end_matches('-')
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase();
    match cleaned.as_str() {
        "choose one" | "one" => Ok((1, 1)),
        "choose two" | "two" => Ok((2, 2)),
        "choose three" | "three" => Ok((3, 3)),
        "choose four" | "four" => Ok((4, 4)),
        "choose five" | "five" => Ok((5, 5)),
        "choose up to five" | "up to five" => Ok((0, 5)),
        "choose up to one" | "up to one" => Ok((0, 1)),
        "choose up to two" | "up to two" => Ok((0, 2)),
        "choose up to three" | "up to three" => Ok((0, 3)),
        "choose up to four" | "up to four" => Ok((0, 4)),
        "choose one or both" | "one or both" => Ok((1, 2)),
        "choose one or two" | "one or two" => Ok((1, 2)),
        "choose one, two, or three" | "one, two, or three" => Ok((1, 3)),
        "choose two or more" | "two or more" => Ok((2, 255)),
        "choose one or more" | "one or more" => Ok((1, 255)),
        "choose any number" | "any_number" | "any number" => Ok((0, 255)),
        _ => Err("oracle:unsupported-modal-choice"),
    }
}

pub(super) fn rewrite_value_expr_anaphora(expr: &mut ValueExpr, replacement: ValueObjectRef) {
    match expr {
        ValueExpr::ObjectStat { subject, .. } if *subject == ValueObjectRef::TargetObject => {
            *subject = replacement;
        }
        ValueExpr::CounterCount { subject, .. }
            if *subject == spellwake_engine::program::CounterSubjectRef::TargetObject =>
        {
            if let Some(cs) = match replacement {
                ValueObjectRef::DyingSource => {
                    Some(spellwake_engine::program::CounterSubjectRef::DyingSource)
                }
                ValueObjectRef::TriggerSource => {
                    Some(spellwake_engine::program::CounterSubjectRef::TriggerSource)
                }
                ValueObjectRef::DyingCreature
                | ValueObjectRef::EnteringPermanent
                | ValueObjectRef::TriggerSubject => {
                    Some(spellwake_engine::program::CounterSubjectRef::TriggerSubject)
                }
                ValueObjectRef::AbilitySource => {
                    Some(spellwake_engine::program::CounterSubjectRef::AbilitySource)
                }
                ValueObjectRef::AttachedHost => {
                    Some(spellwake_engine::program::CounterSubjectRef::AttachedHost)
                }
                _ => None,
            } {
                *subject = cs;
            }
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            rewrite_value_expr_anaphora(inner, replacement);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_value_expr_anaphora(a, replacement);
            rewrite_value_expr_anaphora(b, replacement);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_value_expr_anaphora(left, replacement);
            rewrite_value_expr_anaphora(right, replacement);
            rewrite_value_expr_anaphora(then_value, replacement);
            rewrite_value_expr_anaphora(else_value, replacement);
        }
        _ => {}
    }
}

pub(super) fn rewrite_instruction_anaphora(op: &mut Instruction, replacement: ValueObjectRef) {
    match op {
        Instruction::AddMana(
            EffectManaYield::Counted { amount, .. }
            | EffectManaYield::Split { amount, .. }
            | EffectManaYield::Choice { amount, .. }
            | EffectManaYield::RestrictedChoice { amount, .. }
            | EffectManaYield::LandProducible { amount, .. }
            | EffectManaYield::CommanderIdentity { amount }
            | EffectManaYield::DistinctColors { amount }
            | EffectManaYield::SourceChosenColor { amount },
        )
        | Instruction::GainControllerLife { amount }
        | Instruction::DrainTargetAndGain(amount)
        | Instruction::OpponentsLoseAndControllerGains(amount)
        | Instruction::DrawController { amount }
        | Instruction::DrawTargetPlayer { amount }
        | Instruction::DrawControllerAndTarget { amount }
        | Instruction::DrawEachPlayer { amount }
        | Instruction::Mill { amount, .. }
        | Instruction::Scry { amount, .. }
        | Instruction::Surveil { amount, .. }
        | Instruction::DamageAmount(amount)
        | Instruction::DamageAmountThenExileIfDies(amount)
        | Instruction::DamageOpponentsAmount(amount)
        | Instruction::DamageRelatedPlayerAmount(amount)
        | Instruction::DamageGroupAmount { amount, .. }
        | Instruction::DrainOpponents(amount)
        | Instruction::Discard { amount, .. }
        | Instruction::DiscardFiltered { amount, .. }
        | Instruction::Sacrifice { amount, .. }
        | Instruction::ReturnControlledPermanents { amount, .. }
        | Instruction::AddPlayerCounter { amount, .. }
        | Instruction::PutNamedCounterOnTarget { amount, .. }
        | Instruction::PutCountersOnSubject { amount, .. }
        | Instruction::AdjustPlayersLife { amount, .. }
        | Instruction::RemoveCountersFromPermanent { amount, .. }
        | Instruction::ExileTopPlay { count: amount, .. }
        | Instruction::LookTopChoose { count: amount, .. }
        | Instruction::ModifyGroupByAmount { amount, .. }
        | Instruction::BoostTargetPower { amount, .. }
        | Instruction::Incubate { count: amount }
        | Instruction::Discover { value: amount }
        | Instruction::Connive { count: amount, .. }
        | Instruction::Blight { amount, .. }
        | Instruction::CollectEvidence {
            minimum: amount, ..
        }
        | Instruction::PayLife { amount, .. }
        | Instruction::PayEnergy { amount, .. }
        | Instruction::TapOrUntapControlled { amount, .. }
        | Instruction::ExileFromZone { amount, .. }
        | Instruction::CreateTokenCopy { count: amount, .. }
        | Instruction::Monstrosity { amount }
        | Instruction::OfferDynamicManaPayment {
            extra_generic: amount,
            ..
        }
        | Instruction::RepeatLoop { count: amount, .. }
        | Instruction::AffectPermanentTargets {
            action: PermanentTargetAction::PutCounters { amount, .. },
            ..
        } => {
            rewrite_value_expr_anaphora(amount, replacement);
        }
        Instruction::DealDamageFrom { amount, .. } => {
            if let spellwake_engine::program::DamageAmountSpec::Expr(expr) = amount.as_mut() {
                rewrite_value_expr_anaphora(expr, replacement);
            }
        }
        Instruction::SelectZoneCardsToZone { min, max, .. } => {
            rewrite_value_expr_anaphora(min, replacement);
            rewrite_value_expr_anaphora(max, replacement);
        }
        Instruction::CreateContinuousEffect { parts, .. }
        | Instruction::ChangeAbilitySource { parts, .. }
        | Instruction::ChangeGroup { parts, .. } => {
            for part in &mut parts.0 {
                match part {
                    ResolvedPartSpec::ModifyPTExpr { power, toughness }
                    | ResolvedPartSpec::SetPTExpr { power, toughness } => {
                        rewrite_value_expr_anaphora(power, replacement);
                        rewrite_value_expr_anaphora(toughness, replacement);
                    }
                    _ => {}
                }
            }
        }
        Instruction::CreateTokens { creations }
        | Instruction::CreateAttackingTokens { creations } => {
            for spec in creations {
                rewrite_value_expr_anaphora(&mut spec.amount, replacement);
                if let Some(TokenSize::AtCreation(expr)) = &mut spec.base_size {
                    rewrite_value_expr_anaphora(expr, replacement);
                }
            }
        }
        Instruction::BranchIfFalse { condition, .. } => {
            rewrite_value_expr_anaphora(&mut condition.left, replacement);
            rewrite_value_expr_anaphora(&mut condition.right, replacement);
        }
        Instruction::EmitReflexiveTrigger { body }
        | Instruction::ScheduleDelayedStepTrigger { body, .. } => {
            rewrite_body_anaphora_stats(body, replacement);
        }
        _ => {}
    }
}

pub(super) fn rewrite_body_anaphora_stats(body: &mut ExecutableBody, replacement: ValueObjectRef) {
    let allow_rewrite = matches!(
        body.target,
        BodyTarget::None
            | BodyTarget::SinglePlayer
            | BodyTarget::SingleOpponent
            | BodyTarget::SingleSpell(_)
            | BodyTarget::SingleAnyTarget
            | BodyTarget::SingleCreatureOrPlayer
            | BodyTarget::SinglePlayerOrPlaneswalker
            | BodyTarget::SingleOpponentOrPlaneswalker
    );
    if !allow_rewrite {
        return;
    }
    for op in &mut body.instructions {
        rewrite_instruction_anaphora(op, replacement);
    }
}

pub(super) fn rewrite_value_expr_prior_result(expr: &mut ValueExpr, replacement: EventValueKind) {
    match expr {
        ValueExpr::EventValue(kind) if *kind == EventValueKind::LastActionCount => {
            *kind = replacement;
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            rewrite_value_expr_prior_result(inner, replacement);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_value_expr_prior_result(a, replacement);
            rewrite_value_expr_prior_result(b, replacement);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_value_expr_prior_result(left, replacement);
            rewrite_value_expr_prior_result(right, replacement);
            rewrite_value_expr_prior_result(then_value, replacement);
            rewrite_value_expr_prior_result(else_value, replacement);
        }
        _ => {}
    }
}

pub(super) fn rewrite_body_prior_result(body: &mut ExecutableBody, replacement: EventValueKind) {
    let mut saw_count_action = false;
    for op in &mut body.instructions {
        if !saw_count_action {
            match op {
                Instruction::AddMana(
                    EffectManaYield::Counted { amount, .. }
                    | EffectManaYield::Split { amount, .. }
                    | EffectManaYield::Choice { amount, .. }
                    | EffectManaYield::RestrictedChoice { amount, .. }
                    | EffectManaYield::LandProducible { amount, .. }
                    | EffectManaYield::CommanderIdentity { amount }
                    | EffectManaYield::DistinctColors { amount }
                    | EffectManaYield::SourceChosenColor { amount },
                )
                | Instruction::GainControllerLife { amount }
                | Instruction::DrainTargetAndGain(amount)
                | Instruction::OpponentsLoseAndControllerGains(amount)
                | Instruction::DrawController { amount }
                | Instruction::DrawTargetPlayer { amount }
                | Instruction::DrawControllerAndTarget { amount }
                | Instruction::DrawEachPlayer { amount }
                | Instruction::Mill { amount, .. }
                | Instruction::Scry { amount, .. }
                | Instruction::Surveil { amount, .. }
                | Instruction::DamageAmount(amount)
                | Instruction::DamageAmountThenExileIfDies(amount)
                | Instruction::DamageOpponentsAmount(amount)
                | Instruction::DamageRelatedPlayerAmount(amount)
                | Instruction::DamageGroupAmount { amount, .. }
                | Instruction::DrainOpponents(amount)
                | Instruction::Discard { amount, .. }
                | Instruction::DiscardFiltered { amount, .. }
                | Instruction::Sacrifice { amount, .. }
                | Instruction::ReturnControlledPermanents { amount, .. }
                | Instruction::AddPlayerCounter { amount, .. }
                | Instruction::PutNamedCounterOnTarget { amount, .. }
                | Instruction::PutCountersOnSubject { amount, .. }
                | Instruction::AdjustPlayersLife { amount, .. }
                | Instruction::RemoveCountersFromPermanent { amount, .. }
                | Instruction::ExileTopPlay { count: amount, .. }
                | Instruction::LookTopChoose { count: amount, .. }
                | Instruction::ModifyGroupByAmount { amount, .. }
                | Instruction::BoostTargetPower { amount, .. } => {
                    rewrite_value_expr_prior_result(amount, replacement);
                }
                Instruction::CreateTokens { creations }
                | Instruction::CreateAttackingTokens { creations } => {
                    for spec in creations {
                        rewrite_value_expr_prior_result(&mut spec.amount, replacement);
                        if let Some(TokenSize::AtCreation(expr)) = &mut spec.base_size {
                            rewrite_value_expr_prior_result(expr, replacement);
                        }
                    }
                }
                Instruction::BranchIfFalse { condition, .. } => {
                    rewrite_value_expr_prior_result(&mut condition.left, replacement);
                    rewrite_value_expr_prior_result(&mut condition.right, replacement);
                }
                Instruction::EmitReflexiveTrigger { body: inner }
                | Instruction::ScheduleDelayedStepTrigger { body: inner, .. } => {
                    rewrite_body_prior_result(inner, replacement);
                }
                _ => {}
            }
        }
        if matches!(
            op,
            Instruction::Discard { .. }
                | Instruction::DiscardFiltered { .. }
                | Instruction::DiscardUpTo { .. }
                | Instruction::Mill { .. }
                | Instruction::Sacrifice { .. }
                | Instruction::SacrificeMatching(_)
                | Instruction::DestroyGroup(_)
                | Instruction::DestroyGroupWithoutRegeneration(_)
                | Instruction::MoveGroup { .. }
        ) {
            saw_count_action = true;
        }
    }
}

pub(super) fn parse_cast_timing_restriction(
    restriction: &str,
    condition: Option<&ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::program::CastTimingRestriction> {
    use spellwake_engine::program::CastTimingRestriction as R;
    use spellwake_engine::turn::Step;
    let cond_rule = if let Some(cond) = condition {
        let mut dummy = BodyBuilder::new();
        let rc = lower_resolution_condition(cond, &mut dummy, vocabulary, false, None).ok()?;
        if dummy.target == BodyTarget::None && rc.is_world_evaluable() {
            Some(R::Condition(Box::new(rc)))
        } else {
            return None;
        }
    } else {
        None
    };
    let low = restriction.trim().to_ascii_lowercase();
    if let Some(cond_r) = cond_rule {
        let base_tag = low
            .strip_prefix("conditional+")
            .or_else(|| low.strip_prefix("conditional"))
            .unwrap_or(&low)
            .trim();
        if base_tag.is_empty() || matches!(base_tag, "condition" | "only_if" | "if") {
            return Some(cond_r);
        }
        let timing = parse_cast_timing_restriction(base_tag, None, vocabulary)?;
        return Some(R::All(vec![timing, cond_r]));
    }
    if low.contains('+')
        && low != "during combat+during opponent's turn"
        && low != "during opponent's turn+during combat"
    {
        let mut parts = Vec::new();
        for part in low.split('+') {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            parts.push(parse_cast_timing_restriction(p, None, vocabulary)?);
        }
        return match parts.len() {
            0 => None,
            1 => parts.pop(),
            _ => Some(R::All(parts)),
        };
    }
    if low == "duringcombat"
        || low == "during combat"
        || low == "cast this spell only during combat"
        || low == "cast ~ only during combat"
    {
        return Some(R::DuringCombat);
    }
    if (low.contains("declare attackers step") && low.contains("attacked"))
        || low == "duringdeclareattackersifattacked"
    {
        return Some(R::DuringDeclareAttackersIfAttacked);
    }
    if low.contains("before blockers are declared") || low == "duringcombatbeforeblockers" {
        return Some(R::DuringCombatBeforeBlockers);
    }
    if low.contains("before the combat damage step")
        || low.contains("before combat damage")
        || low == "beforecombatdamage"
    {
        return Some(R::BeforeCombatDamage);
    }
    if low.contains("after blockers are declared") || low == "duringcombatafterblockers" {
        return Some(R::DuringCombatAfterBlockers);
    }
    if low == "during the declare attackers step"
        || low == "cast ~ only during the declare attackers step"
        || low == "cast this spell only during the declare attackers step"
    {
        return Some(R::DuringDeclareAttackers);
    }
    if low == "during the declare blockers step"
        || low == "cast ~ only during the declare blockers step"
        || low == "cast this spell only during the declare blockers step"
    {
        return Some(R::DuringDeclareBlockers);
    }
    if low == "during combat on an opponent's turn"
        || low == "cast ~ only during combat on an opponent's turn"
        || low == "cast this spell only during combat on an opponent's turn"
        || low == "during combat+during opponent's turn"
        || low == "during opponent's turn+during combat"
    {
        return Some(R::DuringCombatOnOpponentsTurn);
    }
    if low == "duringyourturn"
        || low == "during your turn"
        || low == "cast ~ only during your turn"
        || low == "cast this spell only during your turn"
    {
        return Some(R::DuringYourTurn);
    }
    if low == "duringopponentsturn"
        || low == "during an opponent's turn"
        || low == "cast ~ only during an opponent's turn"
        || low == "cast this spell only during an opponent's turn"
    {
        return Some(R::DuringOpponentsTurn);
    }
    if low == "duringupkeep"
        || low == "during your upkeep"
        || low == "cast ~ only during your upkeep"
        || low == "cast this spell only during your upkeep"
    {
        return Some(R::DuringOwnStep(Step::Upkeep));
    }
    if low == "during an opponent's upkeep"
        || low == "cast ~ only during an opponent's upkeep"
        || low == "cast this spell only during an opponent's upkeep"
    {
        return Some(R::DuringOpponentsStep(Step::Upkeep));
    }
    if low == "during upkeep"
        || low == "during any upkeep"
        || low == "cast ~ only during upkeep"
        || low == "cast this spell only during upkeep"
    {
        return Some(R::DuringStep(Step::Upkeep));
    }
    if low == "during your draw step" || low == "cast ~ only during your draw step" {
        return Some(R::DuringOwnStep(Step::Draw));
    }
    if low == "during your end step" || low == "cast ~ only during your end step" {
        return Some(R::DuringOwnStep(Step::End));
    }
    if low == "during an opponent's end step" || low == "cast ~ only during an opponent's end step"
    {
        return Some(R::DuringOpponentsStep(Step::End));
    }
    None
}

pub(super) fn strip_triggered_restrictions(
    body: &[InstructionAst],
    spec: &mut TriggerSpec,
    grouping: &mut TriggerGrouping,
    vocabulary: &CreatureTypeVocabulary,
) -> Vec<InstructionAst> {
    let apply_trig_tag = |tag: &str,
                          cond_acc: &mut Option<TriggerCondition>,
                          grp: &mut TriggerGrouping,
                          matched_any: &mut bool|
     -> bool {
        let trimmed = tag.trim();
        if trimmed.is_empty()
            || trimmed == "Conditional"
            || trimmed == "Condition"
            || trimmed.eq_ignore_ascii_case("only_if")
            || trimmed.eq_ignore_ascii_case("if")
        {
            return true;
        }
        let low = trimmed.to_ascii_lowercase();
        let push_cond = |acc: &mut Option<TriggerCondition>, next: TriggerCondition| {
            *acc = Some(match acc.take() {
                None => next,
                Some(existing) => TriggerCondition::And(Box::new(existing), Box::new(next)),
            });
        };
        if trimmed == "OnceEachTurn"
            || trimmed == "TriggersOnceEachTurn"
            || low == "once each turn"
            || low == "once per turn"
            || low == "only once each turn"
            || low == "do this only once each turn"
            || low == "this ability triggers only once each turn"
        {
            *grp = TriggerGrouping::OneOrMoreOnceEachTurn;
            *matched_any = true;
            return true;
        }
        if trimmed == "DuringYourTurnOnceEachTurn" {
            *grp = TriggerGrouping::OneOrMoreOnceEachTurn;
            push_cond(cond_acc, TriggerCondition::ControllersTurn);
            *matched_any = true;
            return true;
        }
        if trimmed == "DuringYourTurn" || low == "during your turn" {
            push_cond(cond_acc, TriggerCondition::ControllersTurn);
            *matched_any = true;
            return true;
        }
        if trimmed == "DuringOpponentsTurn"
            || low == "during an opponent's turn"
            || low == "during each opponent's turn"
            || low == "during opponents' turns"
        {
            push_cond(cond_acc, TriggerCondition::OpponentsTurn);
            *matched_any = true;
            return true;
        }
        if trimmed == "DuringUpkeep" || low == "during your upkeep" {
            push_cond(
                cond_acc,
                TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(TriggerCondition::DuringStep(Step::Upkeep)),
                ),
            );
            *matched_any = true;
            return true;
        }
        if low == "during an opponent's upkeep" {
            push_cond(
                cond_acc,
                TriggerCondition::And(
                    Box::new(TriggerCondition::OpponentsTurn),
                    Box::new(TriggerCondition::DuringStep(Step::Upkeep)),
                ),
            );
            *matched_any = true;
            return true;
        }
        if low == "during upkeep" || low == "during any upkeep" {
            push_cond(cond_acc, TriggerCondition::DuringStep(Step::Upkeep));
            *matched_any = true;
            return true;
        }
        if trimmed == "DuringCombat" || low == "during combat" {
            let combat_cond = TriggerCondition::Or(
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
            );
            push_cond(cond_acc, combat_cond);
            *matched_any = true;
            return true;
        }
        false
    };

    let mut stripped_body = Vec::with_capacity(body.len());
    for ins in body {
        if let InstructionAst::AbilityRestriction {
            restriction,
            condition,
        } = ins
        {
            let mut local_cond: Option<TriggerCondition> = None;
            let mut local_grp = *grouping;
            let mut matched_any = false;
            let base_raw = if condition.is_some() {
                restriction
                    .strip_prefix("Conditional")
                    .unwrap_or(restriction.as_str())
            } else {
                restriction.as_str()
            };
            let tags_ok = base_raw.split('+').all(|part| {
                apply_trig_tag(part, &mut local_cond, &mut local_grp, &mut matched_any)
            });
            if tags_ok {
                let cond_ok = if let Some(cond) = condition {
                    if let Ok(tc) = lower_trigger_condition(cond, vocabulary) {
                        local_cond = Some(match local_cond.take() {
                            None => tc,
                            Some(existing) => {
                                TriggerCondition::And(Box::new(existing), Box::new(tc))
                            }
                        });
                        true
                    } else {
                        false
                    }
                } else {
                    true
                };
                if cond_ok && (matched_any || local_cond.is_some()) {
                    *grouping = local_grp;
                    if let Some(tc) = local_cond {
                        let combined = match spec.intervening_if.take() {
                            None => tc,
                            Some(existing) => {
                                TriggerCondition::And(Box::new(existing), Box::new(tc))
                            }
                        };
                        *spec = spec.clone().only_if(combined);
                    }
                    continue;
                }
            }
        }
        match ins {
            InstructionAst::Optional(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::Optional(s));
                continue;
            }
            InstructionAst::IfPreviousSucceeded(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::IfPreviousSucceeded(s));
                continue;
            }
            InstructionAst::IfPreviousFailed(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::IfPreviousFailed(s));
                continue;
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => {
                let t = strip_triggered_restrictions(then_body, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::IfCondition {
                    condition: condition.clone(),
                    then_body: t,
                });
                continue;
            }
            InstructionAst::BindX {
                x_expr,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::BindX {
                    x_expr: x_expr.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::OptionalPlayer {
                player,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::OptionalPlayer {
                    player: *player,
                    body: s,
                });
                continue;
            }
            InstructionAst::Otherwise(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::Otherwise(s));
                continue;
            }
            InstructionAst::ReplaceEffect(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ReplaceEffect(s));
                continue;
            }
            InstructionAst::ForEachPlayer {
                player,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForEachPlayer {
                    player: *player,
                    body: s,
                });
                continue;
            }
            InstructionAst::ForEachObject {
                filter,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForEachObject {
                    filter: filter.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::ForEachCount { count, body: inner } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForEachCount {
                    count: count.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::Unless {
                body: inner,
                unless_player,
                unless_cost,
                unless_action,
                unless_condition,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::Unless {
                    body: s,
                    unless_player: *unless_player,
                    unless_cost: unless_cost.clone(),
                    unless_action: unless_action.clone(),
                    unless_condition: unless_condition.clone(),
                });
                continue;
            }
            InstructionAst::Modal { choice, modes } => {
                let new_modes = modes
                    .iter()
                    .map(|m| spellwake_oracle_compiler::ast::ModalModeAst {
                        body: strip_triggered_restrictions(&m.body, spec, grouping, vocabulary),
                    })
                    .collect();
                stripped_body.push(InstructionAst::Modal {
                    choice: choice.clone(),
                    modes: new_modes,
                });
                continue;
            }
            _ => {}
        }
        stripped_body.push(ins.clone());
    }
    stripped_body
}

pub(super) fn body_returns_self_from_graveyard(body: &[InstructionAst]) -> bool {
    for ins in body {
        match ins {
            InstructionAst::ReturnZone {
                subject,
                from_zone: Some(from),
                ..
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && from.to_ascii_lowercase().contains("graveyard") =>
            {
                return true;
            }
            InstructionAst::Optional(inner) | InstructionAst::IfPreviousSucceeded(inner)
                if body_returns_self_from_graveyard(inner) =>
            {
                return true;
            }
            InstructionAst::IfCondition { then_body, .. }
                if body_returns_self_from_graveyard(then_body) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(super) fn body_references_self_from_graveyard(body: &[InstructionAst]) -> bool {
    for ins in body {
        match ins {
            InstructionAst::ReturnZone {
                subject,
                from_zone: Some(from),
                ..
            } if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && from.to_ascii_lowercase().contains("graveyard") =>
            {
                return true;
            }
            InstructionAst::MoveZone { subject, to }
                if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                    && to.to_ascii_lowercase().contains("hand") =>
            {
                return true;
            }
            InstructionAst::Exile { subject, .. }
                if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
            {
                return true;
            }
            InstructionAst::ExileGraveyard { whose }
                if whose.contains('~') || whose.to_ascii_lowercase().contains("this card") =>
            {
                return true;
            }
            InstructionAst::Optional(inner) | InstructionAst::IfPreviousSucceeded(inner)
                if body_references_self_from_graveyard(inner) =>
            {
                return true;
            }
            InstructionAst::IfCondition { then_body, .. }
                if body_references_self_from_graveyard(then_body) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn rewrite_value_expr_trigger_subject(expr: &mut ValueExpr, replacement: ValueObjectRef) {
    match expr {
        ValueExpr::ObjectStat { subject, .. } if *subject == ValueObjectRef::DyingCreature => {
            *subject = replacement;
        }
        ValueExpr::CounterCount { subject, .. }
            if *subject == spellwake_engine::program::CounterSubjectRef::TriggerSubject =>
        {
            if let Some(cs) = match replacement {
                ValueObjectRef::DyingSource => {
                    Some(spellwake_engine::program::CounterSubjectRef::DyingSource)
                }
                ValueObjectRef::TriggerSource => {
                    Some(spellwake_engine::program::CounterSubjectRef::TriggerSource)
                }
                ValueObjectRef::AbilitySource => {
                    Some(spellwake_engine::program::CounterSubjectRef::AbilitySource)
                }
                _ => None,
            } {
                *subject = cs;
            }
        }
        ValueExpr::Scale { expr: inner, .. } | ValueExpr::DivConst { expr: inner, .. } => {
            rewrite_value_expr_trigger_subject(inner, replacement);
        }
        ValueExpr::Add(a, b) | ValueExpr::Sub(a, b) => {
            rewrite_value_expr_trigger_subject(a, replacement);
            rewrite_value_expr_trigger_subject(b, replacement);
        }
        ValueExpr::Conditional {
            left,
            right,
            then_value,
            else_value,
            ..
        } => {
            rewrite_value_expr_trigger_subject(left, replacement);
            rewrite_value_expr_trigger_subject(right, replacement);
            rewrite_value_expr_trigger_subject(then_value, replacement);
            rewrite_value_expr_trigger_subject(else_value, replacement);
        }
        _ => {}
    }
}

fn rewrite_body_trigger_subject_stats(body: &mut ExecutableBody, replacement: ValueObjectRef) {
    for op in &mut body.instructions {
        match op {
            Instruction::AddMana(
                EffectManaYield::Counted { amount, .. }
                | EffectManaYield::Split { amount, .. }
                | EffectManaYield::Choice { amount, .. }
                | EffectManaYield::RestrictedChoice { amount, .. }
                | EffectManaYield::LandProducible { amount, .. }
                | EffectManaYield::CommanderIdentity { amount }
                | EffectManaYield::DistinctColors { amount }
                | EffectManaYield::SourceChosenColor { amount },
            )
            | Instruction::GainControllerLife { amount }
            | Instruction::DrainTargetAndGain(amount)
            | Instruction::OpponentsLoseAndControllerGains(amount)
            | Instruction::DrawController { amount }
            | Instruction::DrawTargetPlayer { amount }
            | Instruction::DrawControllerAndTarget { amount }
            | Instruction::DrawEachPlayer { amount }
            | Instruction::Mill { amount, .. }
            | Instruction::Scry { amount, .. }
            | Instruction::Surveil { amount, .. }
            | Instruction::DamageAmount(amount)
            | Instruction::DamageAmountThenExileIfDies(amount)
            | Instruction::DamageOpponentsAmount(amount)
            | Instruction::DamageRelatedPlayerAmount(amount)
            | Instruction::DamageGroupAmount { amount, .. }
            | Instruction::DrainOpponents(amount)
            | Instruction::Discard { amount, .. }
            | Instruction::DiscardFiltered { amount, .. }
            | Instruction::Sacrifice { amount, .. }
            | Instruction::ReturnControlledPermanents { amount, .. }
            | Instruction::AddPlayerCounter { amount, .. }
            | Instruction::PutNamedCounterOnTarget { amount, .. }
            | Instruction::PutCountersOnSubject { amount, .. }
            | Instruction::AdjustPlayersLife { amount, .. }
            | Instruction::RemoveCountersFromPermanent { amount, .. }
            | Instruction::ExileTopPlay { count: amount, .. }
            | Instruction::LookTopChoose { count: amount, .. }
            | Instruction::ModifyGroupByAmount { amount, .. }
            | Instruction::BoostTargetPower { amount, .. }
            | Instruction::Incubate { count: amount }
            | Instruction::Discover { value: amount }
            | Instruction::Connive { count: amount, .. }
            | Instruction::Blight { amount, .. }
            | Instruction::CollectEvidence {
                minimum: amount, ..
            }
            | Instruction::PayLife { amount, .. }
            | Instruction::PayEnergy { amount, .. }
            | Instruction::TapOrUntapControlled { amount, .. }
            | Instruction::ExileFromZone { amount, .. }
            | Instruction::CreateTokenCopy { count: amount, .. }
            | Instruction::Monstrosity { amount }
            | Instruction::OfferDynamicManaPayment {
                extra_generic: amount,
                ..
            }
            | Instruction::RepeatLoop { count: amount, .. }
            | Instruction::AffectPermanentTargets {
                action: PermanentTargetAction::PutCounters { amount, .. },
                ..
            } => {
                rewrite_value_expr_trigger_subject(amount, replacement);
            }
            Instruction::DealDamageFrom { amount, .. } => {
                if let spellwake_engine::program::DamageAmountSpec::Expr(expr) = amount.as_mut() {
                    rewrite_value_expr_trigger_subject(expr, replacement);
                }
            }
            Instruction::SelectZoneCardsToZone { min, max, .. } => {
                rewrite_value_expr_trigger_subject(min, replacement);
                rewrite_value_expr_trigger_subject(max, replacement);
            }
            Instruction::CreateContinuousEffect { parts, .. }
            | Instruction::ChangeAbilitySource { parts, .. }
            | Instruction::ChangeGroup { parts, .. } => {
                for part in &mut parts.0 {
                    match part {
                        ResolvedPartSpec::ModifyPTExpr { power, toughness }
                        | ResolvedPartSpec::SetPTExpr { power, toughness } => {
                            rewrite_value_expr_trigger_subject(power, replacement);
                            rewrite_value_expr_trigger_subject(toughness, replacement);
                        }
                        _ => {}
                    }
                }
            }
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations } => {
                for spec in creations {
                    rewrite_value_expr_trigger_subject(&mut spec.amount, replacement);
                    if let Some(TokenSize::AtCreation(expr)) = &mut spec.base_size {
                        rewrite_value_expr_trigger_subject(expr, replacement);
                    }
                }
            }
            Instruction::BranchIfFalse { condition, .. } => {
                rewrite_value_expr_trigger_subject(&mut condition.left, replacement);
                rewrite_value_expr_trigger_subject(&mut condition.right, replacement);
            }
            Instruction::EmitReflexiveTrigger { body: inner }
            | Instruction::ScheduleDelayedStepTrigger { body: inner, .. } => {
                rewrite_body_trigger_subject_stats(inner, replacement);
            }
            _ => {}
        }
    }
    if let Some(modes) = body.triggered_modes_mut() {
        for opt in &mut modes.options {
            rewrite_body_trigger_subject_stats(opt, replacement);
        }
    }
}

pub(super) fn apply_trigger_body_rewrites(
    exec_body: &mut ExecutableBody,
    spec: &TriggerSpec,
    grouping: TriggerGrouping,
    self_pronoun: bool,
) {
    let apply_one = |b: &mut ExecutableBody| {
        if let TriggerEvent::Dies {
            subject,
            mode: DiesTriggerMode::Creature | DiesTriggerMode::DamagedBySource,
        } = &spec.event
        {
            if *subject == ObjectFilter::self_object() {
                rewrite_body_anaphora_stats(b, ValueObjectRef::DyingSource);
                rewrite_body_trigger_subject_stats(b, ValueObjectRef::DyingSource);
            } else {
                rewrite_body_anaphora_stats(b, ValueObjectRef::DyingCreature);
            }
        } else if matches!(spec.event, TriggerEvent::Enters { .. }) && !self_pronoun {
            rewrite_body_anaphora_stats(b, ValueObjectRef::EnteringPermanent);
            rewrite_body_trigger_subject_stats(b, ValueObjectRef::EnteringPermanent);
        } else if matches!(spec.event, TriggerEvent::SpellCast { .. }) {
            rewrite_body_anaphora_stats(b, ValueObjectRef::TriggeringSpell);
            rewrite_body_trigger_subject_stats(b, ValueObjectRef::TriggeringSpell);
        } else if matches!(
            spec.event,
            TriggerEvent::CountersChanged {
                kind: CounterTriggerKind::SagaChapter(_),
                ..
            }
        ) {
            rewrite_body_trigger_subject_stats(b, ValueObjectRef::FinalSaga);
        } else if self_pronoun {
            rewrite_body_anaphora_stats(b, ValueObjectRef::TriggerSource);
            rewrite_body_trigger_subject_stats(b, ValueObjectRef::TriggerSource);
        } else {
            rewrite_body_trigger_subject_stats(b, ValueObjectRef::TriggerSubject);
            if b.target == BodyTarget::None {
                rewrite_body_anaphora_stats(b, ValueObjectRef::TriggerSubject);
            }
        }
        if grouping == TriggerGrouping::EachOccurrence {
            match &spec.event {
                TriggerEvent::DealsDamage {
                    combat_only: true, ..
                } => {
                    rewrite_body_prior_result(b, EventValueKind::CombatDamageToPlayer);
                }
                TriggerEvent::DealtDamage {
                    linked_player: false,
                    ..
                } => {
                    rewrite_body_prior_result(b, EventValueKind::DamageReceived);
                }
                TriggerEvent::LifeChanged { .. } => {
                    rewrite_body_prior_result(b, EventValueKind::TriggeringLifeChange);
                }
                _ => {}
            }
        }
    };
    apply_one(exec_body);
    if let Some(modes) = exec_body.triggered_modes_mut() {
        for opt in &mut modes.options {
            apply_one(opt);
        }
    }
}

pub(super) fn combine_activation_restrictions(
    existing: Option<ActivationRestriction>,
    next_r: ActivationRestriction,
) -> ActivationRestriction {
    match (existing, next_r) {
        (None, r) => r,
        (Some(ActivationRestriction::Sorcery), ActivationRestriction::OnceEachTurn)
        | (Some(ActivationRestriction::OnceEachTurn), ActivationRestriction::Sorcery) => {
            ActivationRestriction::SorceryOnceEachTurn
        }
        (Some(ActivationRestriction::DuringYourTurn), ActivationRestriction::OnceEachTurn)
        | (Some(ActivationRestriction::OnceEachTurn), ActivationRestriction::DuringYourTurn) => {
            ActivationRestriction::DuringYourTurnOnceEachTurn
        }
        (Some(ActivationRestriction::All(mut items)), ActivationRestriction::All(more)) => {
            items.extend(more);
            ActivationRestriction::All(items)
        }
        (Some(ActivationRestriction::All(mut items)), r) => {
            items.push(r);
            ActivationRestriction::All(items)
        }
        (Some(a), ActivationRestriction::All(mut items)) => {
            items.insert(0, a);
            ActivationRestriction::All(items)
        }
        (Some(a), b) => ActivationRestriction::All(vec![a, b]),
    }
}

pub(super) fn strip_activated_restrictions(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(Vec<InstructionAst>, Option<ActivationRestriction>), &'static str> {
    let mut restriction = None;
    let mut filtered = Vec::with_capacity(ins.len());
    for item in ins {
        match item {
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if r == "Instant" || r.eq_ignore_ascii_case("as an instant") => {
                // Normal ActivatedAbility already enforces instant-speed priority timing.
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if r.eq_ignore_ascii_case("it's still a land")
                || r.eq_ignore_ascii_case("they're still lands")
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
                let next_r = parse_activation_restriction(r)?;
                restriction = Some(combine_activation_restrictions(restriction, next_r));
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: Some(cond),
            } => {
                let base = r.strip_prefix("Conditional").unwrap_or(r.as_str()).trim();
                if !base.is_empty()
                    && base != "Condition"
                    && !base.eq_ignore_ascii_case("only_if")
                    && !base.eq_ignore_ascii_case("if")
                    && base != "Instant"
                    && !base.eq_ignore_ascii_case("as an instant")
                {
                    let base_r = parse_activation_restriction(base)?;
                    restriction = Some(combine_activation_restrictions(restriction, base_r));
                }
                let next_r = lower_conditional_activation_restriction(cond, vocabulary)?;
                restriction = Some(combine_activation_restrictions(restriction, next_r));
            }
            InstructionAst::BindX { x_expr, body } if !body.is_empty() => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(body, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::BindX {
                    x_expr: x_expr.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::Optional(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::Optional(inner_filtered));
            }
            InstructionAst::IfPreviousSucceeded(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::IfPreviousSucceeded(inner_filtered));
            }
            InstructionAst::IfPreviousFailed(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::IfPreviousFailed(inner_filtered));
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => {
                let (t, tr) = strip_activated_restrictions(then_body, vocabulary)?;
                if let Some(ir) = tr {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::IfCondition {
                    condition: condition.clone(),
                    then_body: t,
                });
            }
            InstructionAst::OptionalPlayer {
                player,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::OptionalPlayer {
                    player: *player,
                    body: inner_filtered,
                });
            }
            InstructionAst::Otherwise(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::Otherwise(inner_filtered));
            }
            InstructionAst::ReplaceEffect(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ReplaceEffect(inner_filtered));
            }
            InstructionAst::ForEachPlayer {
                player,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
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
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachObject {
                    filter: filter.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachCount { count, body: inner } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachCount {
                    count: count.clone(),
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
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::Unless {
                    body: inner_filtered,
                    unless_player: *unless_player,
                    unless_cost: unless_cost.clone(),
                    unless_action: unless_action.clone(),
                    unless_condition: unless_condition.clone(),
                });
            }
            InstructionAst::Modal { choice, modes } => {
                let mut new_modes = Vec::with_capacity(modes.len());
                for m in modes {
                    let (mb, mr) = strip_activated_restrictions(&m.body, vocabulary)?;
                    if let Some(ir) = mr {
                        restriction = Some(combine_activation_restrictions(restriction, ir));
                    }
                    new_modes.push(spellwake_oracle_compiler::ast::ModalModeAst { body: mb });
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
    }
    Ok((filtered, restriction))
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

pub(super) fn lower_conditional_activation_restriction(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ActivationRestriction, &'static str> {
    match cond {
        ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ActivationRestriction::CreatedTokenThisTurn),
        ConditionAst::Marker(GameMarker::CitysBlessing) => {
            Ok(ActivationRestriction::HasCityBlessing)
        }
        ConditionAst::HandCount {
            player: PlayerRefAst::You,
            op: CompareOp::Eq,
            value: 0,
        } => Ok(ActivationRestriction::EmptyHand),
        ConditionAst::Morbid {
            your_control_only: false,
        } => Ok(ActivationRestriction::CreatureDiedThisTurn),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value: 1,
        } if filter.any_types == AstCardTypeMask::CREATURE
            && filter.forbidden_types.is_empty()
            && filter.subtypes.is_empty()
            && filter.stat_bounds.len() == 1
            && filter.stat_bounds[0].stat == StatKind::Power
            && filter.stat_bounds[0].op == CompareOp::Ge
            && matches!(filter.stat_bounds[0].value.as_ref(), ValueExprAst::Const(4)) =>
        {
            Ok(ActivationRestriction::ControlsCreaturePowerFourOrGreater)
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
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            let ct = vocabulary
                .resolve_label(&filter.subtypes[0])
                .ok_or("oracle:unsupported-act-restriction")?;
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
            let minimum = NonZeroU8::new(min_u8).ok_or("oracle:unsupported-act-restriction")?;
            Ok(ActivationRestriction::ControlsSubtypeCount { kind: ct, minimum })
        }
        ConditionAst::Threshold => Ok(ActivationRestriction::Threshold),
        ConditionAst::Delirium => Ok(ActivationRestriction::Delirium),
        ConditionAst::Metalcraft => Ok(ActivationRestriction::Metalcraft),
        ConditionAst::IsYourTurn => Ok(ActivationRestriction::DuringYourTurn),
        ConditionAst::Not(inner) if matches!(inner.as_ref(), ConditionAst::IsYourTurn) => {
            Ok(ActivationRestriction::DuringOpponentsTurn)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("tapped") =>
        {
            Ok(ActivationRestriction::SourceIsTapped)
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                && status.eq_ignore_ascii_case("untapped") =>
        {
            Ok(ActivationRestriction::SourceIsUntapped)
        }
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Ge,
            value,
        } if *value >= 0 => Ok(ActivationRestriction::ControllerLifeAtLeast(
            u16::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?,
        )),
        ConditionAst::LifeCompare {
            player: PlayerRefAst::You,
            op: CompareOp::Le,
            value,
        } if *value >= 0 => Ok(ActivationRestriction::ControllerLifeAtMost(
            u16::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?,
        )),
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0
            && filter.subtypes.is_empty()
            && filter.forbidden_types.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.stat_bounds.is_empty()
            && filter.flags.is_empty() =>
        {
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                let min_u8 =
                    u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
                return Ok(ActivationRestriction::ControlsCardType {
                    kind,
                    minimum: min_u8,
                });
            }
            Err("oracle:unsupported-act-restriction")
        }
        ConditionAst::ControlsCount {
            player: PlayerRefAst::You,
            filter,
            op: CompareOp::Ge,
            value,
        } if *value > 0 => {
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ActivationRestriction::ControlsMatching {
                filter: obj,
                minimum: min_u8,
            })
        }
        ConditionAst::MatchesFilter { subject, filter }
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ActivationRestriction::SourceMatches(obj))
        }
        other => {
            let mut tmp_builder = BodyBuilder::new();
            let rc = lower_resolution_condition(other, &mut tmp_builder, vocabulary, false, None)
                .map_err(|_| "oracle:unsupported-act-restriction")?;
            if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
                return Err("oracle:unsupported-act-restriction");
            }
            Ok(ActivationRestriction::Condition(Box::new(rc)))
        }
    }
}

pub(super) fn parse_activation_restriction(
    raw: &str,
) -> Result<ActivationRestriction, &'static str> {
    if let Some(rest) = raw
        .strip_prefix("AnyPlayer:")
        .or_else(|| raw.strip_prefix("anyplayer:"))
    {
        let rest_trimmed = rest.trim();
        if rest_trimmed.is_empty() {
            return Ok(ActivationRestriction::AnyPlayer);
        }
        let inner = parse_activation_restriction(rest_trimmed)?;
        return Ok(combine_activation_restrictions(
            Some(ActivationRestriction::AnyPlayer),
            inner,
        ));
    }
    if raw.contains('+') {
        let mut acc: Option<ActivationRestriction> = None;
        for part in raw.split('+') {
            let trimmed = part.trim();
            if trimmed.is_empty()
                || trimmed == "Instant"
                || trimmed.eq_ignore_ascii_case("as an instant")
            {
                continue;
            }
            let next_r = parse_activation_restriction(trimmed)?;
            acc = Some(combine_activation_restrictions(acc, next_r));
        }
        return acc.ok_or("oracle:unsupported-act-restriction");
    }
    match raw {
        "Sorcery" => return Ok(ActivationRestriction::Sorcery),
        "OnceEachTurn" => return Ok(ActivationRestriction::OnceEachTurn),
        "DuringUpkeep" => return Ok(ActivationRestriction::DuringUpkeep),
        "OwnTurnBeforeAttackers" => return Ok(ActivationRestriction::OwnTurnBeforeAttackers),
        "Exhaust" | "Once" => return Ok(ActivationRestriction::Exhaust),
        "TriggersOnceEachTurn" => return Ok(ActivationRestriction::OnceEachTurn),
        "DuringYourTurn" => return Ok(ActivationRestriction::DuringYourTurn),
        "DuringOpponentsTurn" => return Ok(ActivationRestriction::DuringOpponentsTurn),
        "DuringCombat" => return Ok(ActivationRestriction::DuringCombat),
        "SorceryOnceEachTurn" => return Ok(ActivationRestriction::SorceryOnceEachTurn),
        "DuringYourTurnOnceEachTurn" => {
            return Ok(ActivationRestriction::DuringYourTurnOnceEachTurn);
        }
        "AnyPlayer" => return Ok(ActivationRestriction::AnyPlayer),
        _ => {}
    }
    let low = raw.to_ascii_lowercase();
    let has_once = low.contains("once each turn") || low.contains("once per turn");
    if low == "as an instant" {
        return Err("oracle:instant-restriction-handled-above");
    }
    if low.contains("sorcery") && has_once {
        Ok(ActivationRestriction::SorceryOnceEachTurn)
    } else if low.contains("during your turn") && has_once {
        Ok(ActivationRestriction::DuringYourTurnOnceEachTurn)
    } else if low.contains("during your turn") && low.contains("before attackers") {
        Ok(ActivationRestriction::OwnTurnBeforeAttackers)
    } else if low.contains("sorcery") {
        Ok(ActivationRestriction::Sorcery)
    } else if has_once {
        Ok(ActivationRestriction::OnceEachTurn)
    } else if low == "activate only once" || low == "exhaust" || low == "once" {
        Ok(ActivationRestriction::Exhaust)
    } else if low.contains("during your upkeep") {
        Ok(ActivationRestriction::DuringUpkeep)
    } else if low.contains("during your turn") {
        Ok(ActivationRestriction::DuringYourTurn)
    } else if low.contains("opponent's turn") || low.contains("opponents' turns") {
        Ok(ActivationRestriction::DuringOpponentsTurn)
    } else if low.contains("during combat") {
        Ok(ActivationRestriction::DuringCombat)
    } else if low.contains("any player may activate") {
        Ok(ActivationRestriction::AnyPlayer)
    } else if low.contains("creature died this turn") {
        Ok(ActivationRestriction::CreatureDiedThisTurn)
    } else if low.contains("no cards in hand") {
        Ok(ActivationRestriction::EmptyHand)
    } else {
        Err("oracle:unsupported-act-restriction")
    }
}

pub(super) fn trigger_subject_is_self(trig: &TriggerSpecAst) -> bool {
    !trig.events.is_empty()
        && trig.events.iter().all(|ev| match ev {
            TriggerEventAst::ZoneChange { subject, .. }
            | TriggerEventAst::AttacksWithOthers { subject, .. }
            | TriggerEventAst::AttacksUnblocked { subject }
            | TriggerEventAst::Blocks { subject, .. }
            | TriggerEventAst::BecomesBlocked { subject, .. }
            | TriggerEventAst::TapStateChanged { subject, .. }
            | TriggerEventAst::TurnedFaceUp { subject }
            | TriggerEventAst::Transforms { subject }
            | TriggerEventAst::PhaseChanged { subject, .. }
            | TriggerEventAst::Mutates { subject }
            | TriggerEventAst::BecomesTargeted { subject, .. }
            | TriggerEventAst::CountersChanged { subject, .. }
            | TriggerEventAst::Cycled { subject }
            | TriggerEventAst::Exploits { subject }
            | TriggerEventAst::BecomesAttached { subject, .. }
            | TriggerEventAst::Exerted { subject }
            | TriggerEventAst::Crewed { subject }
            | TriggerEventAst::Explores { subject }
            | TriggerEventAst::Connives { subject }
            | TriggerEventAst::BecomesMonstrous { subject }
            | TriggerEventAst::BecomesRenowned { subject }
            | TriggerEventAst::Exiled { subject } => {
                matches!(subject.as_ref(), SubjectRefAst::SelfObject)
            }
            TriggerEventAst::Attacks { subject, .. } => {
                matches!(subject.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            TriggerEventAst::SpellCast { targets, .. } => {
                matches!(targets.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            TriggerEventAst::DamageDealt { source, .. } => {
                matches!(source.as_deref(), Some(SubjectRefAst::SelfObject))
            }
            _ => false,
        })
}

pub(super) fn is_self_pronoun(
    subject: &SubjectRefAst,
    builder: &BodyBuilder,
    self_pronoun_valid: bool,
) -> bool {
    match subject {
        SubjectRefAst::SelfObject => true,
        SubjectRefAst::Anaphora(p) if self_pronoun_valid && builder.target == BodyTarget::None => {
            matches!(
                p.to_ascii_lowercase().as_str(),
                "it" | "this creature"
                    | "this permanent"
                    | "this artifact"
                    | "this enchantment"
                    | "this land"
            )
        }
        _ => false,
    }
}

pub(super) fn lower_comparison(op: CompareOp) -> Comparison {
    match op {
        CompareOp::Eq => Comparison::Equal,
        CompareOp::Lt => Comparison::Less,
        CompareOp::Le => Comparison::LessOrEqual,
        CompareOp::Gt => Comparison::Greater,
        CompareOp::Ge => Comparison::GreaterOrEqual,
    }
}

pub(super) fn negate_comparison(c: Comparison) -> Comparison {
    match c {
        Comparison::Equal => Comparison::NotEqual,
        Comparison::NotEqual => Comparison::Equal,
        Comparison::Less => Comparison::GreaterOrEqual,
        Comparison::LessOrEqual => Comparison::Greater,
        Comparison::Greater => Comparison::LessOrEqual,
        Comparison::GreaterOrEqual => Comparison::Less,
    }
}

pub(super) fn lower_resolution_condition(
    cond: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<ResolutionCondition, &'static str> {
    match cond {
        ConditionAst::Not(inner) => {
            let mut rc =
                lower_resolution_condition(inner, builder, vocabulary, in_trigger, bound_x)?;
            rc.comparison = negate_comparison(rc.comparison);
            Ok(rc)
        }
        ConditionAst::And(a, b) => {
            let ca = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let cb = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            let ba = ValueExpr::Conditional {
                left: Box::new(ca.left),
                comparison: ca.comparison,
                right: Box::new(ca.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            let bb = ValueExpr::Conditional {
                left: Box::new(cb.left),
                comparison: cb.comparison,
                right: Box::new(cb.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::Add(Box::new(ba), Box::new(bb)),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(2),
            })
        }
        ConditionAst::Or(a, b) => {
            let ca = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let cb = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            let ba = ValueExpr::Conditional {
                left: Box::new(ca.left),
                comparison: ca.comparison,
                right: Box::new(ca.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            let bb = ValueExpr::Conditional {
                left: Box::new(cb.left),
                comparison: cb.comparison,
                right: Box::new(cb.right),
                then_value: Box::new(ValueExpr::literal(1)),
                else_value: Box::new(ValueExpr::literal(0)),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::Add(Box::new(ba), Box::new(bb)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Compare { lhs, op, rhs } => {
            let lhs_res = resolve_x(lhs, bound_x);
            let rhs_res = resolve_x(rhs, bound_x);
            bind_value_expr_targets(lhs_res, builder)?;
            bind_value_expr_targets(rhs_res, builder)?;
            Ok(ResolutionCondition {
                left: lower_value_expr(lhs_res, vocabulary)?,
                comparison: lower_comparison(*op),
                right: lower_value_expr(rhs_res, vocabulary)?,
            })
        }
        ConditionAst::ControlsCount {
            player,
            filter,
            op,
            value,
        } => {
            let obj = lower_object_filter(filter, vocabulary)?;
            let left = match player {
                PlayerRefAst::You => {
                    ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj)
                }
                PlayerRefAst::AnyPlayer | PlayerRefAst::EachPlayer => {
                    ValueExpr::count_battlefield(obj)
                }
                PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent => {
                    ValueExpr::count_battlefield(obj.opposing())
                }
                other => {
                    let rp = lower_resolving_player(*other, builder)?;
                    ValueExpr::count_player_battlefield(rp, obj)
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::PlayerCounterCompare {
            player,
            counter_kind,
            op,
            value,
        } => {
            let ck = parse_counter_kind(counter_kind).ok_or("oracle:unsupported-if-condition")?;
            match player {
                PlayerRefAst::You => {
                    let subject = match ck {
                        CounterKind::Experience => {
                            spellwake_engine::program::CounterSubjectRef::ControllerExperience
                        }
                        CounterKind::Energy => {
                            spellwake_engine::program::CounterSubjectRef::ControllerEnergy
                        }
                        CounterKind::Poison => {
                            spellwake_engine::program::CounterSubjectRef::ControllerPoison
                        }
                        _ => return Err("oracle:unsupported-if-condition"),
                    };
                    Ok(ResolutionCondition {
                        left: ValueExpr::counters(subject, Some(ck)),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxCounter(ck)),
                        comparison: lower_comparison(*op),
                        right: ValueExpr::literal(i64::from(*value)),
                    })
                }
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::Metalcraft => Ok(ResolutionCondition {
            left: ValueExpr::controlled_type(CardType::Artifact),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(3),
        }),
        ConditionAst::Threshold => Ok(ResolutionCondition {
            left: ValueExpr::count_graveyard(ResolvingPlayer::Controller, ObjectFilter::any()),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(7),
        }),
        ConditionAst::HandCount { player, op, value } => {
            let left = match player {
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::AnyOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                _ => {
                    let rp = lower_resolving_player(*player, builder)?;
                    ValueExpr::count_hand(rp)
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::GraveyardCount { player, op, value } => {
            let left = match player {
                PlayerRefAst::AnyPlayer if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    let yours = ValueExpr::count_graveyard(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    );
                    let opp_max = ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(opp_max.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_max),
                    }
                }
                PlayerRefAst::AnyPlayer
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    let yours = ValueExpr::count_graveyard(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    );
                    let opp_min = ValueExpr::OpponentMinCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    };
                    ValueExpr::Conditional {
                        left: Box::new(yours.clone()),
                        comparison: Comparison::LessOrEqual,
                        right: Box::new(opp_min.clone()),
                        then_value: Box::new(yours),
                        else_value: Box::new(opp_min),
                    }
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::AnyOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent
                    if matches!(op, CompareOp::Le | CompareOp::Lt)
                        || (*op == CompareOp::Eq && *value == 0) =>
                {
                    ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::OpponentMinCount {
                        zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    }
                }
                _ => {
                    let rp = lower_resolving_player(*player, builder)?;
                    ValueExpr::count_graveyard(rp, ObjectFilter::any())
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::ZoneObjectCount { filter, op, value } => {
            let left = lower_value_expr(&ValueExprAst::Count(filter.clone()), vocabulary)?;
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::LifeCompare { player, op, value } => {
            let left = match player {
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)
                }
                PlayerRefAst::AnyOpponent if matches!(op, CompareOp::Le | CompareOp::Lt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife)
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Le | CompareOp::Lt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife)
                }
                PlayerRefAst::EachOpponent if matches!(op, CompareOp::Ge | CompareOp::Gt) => {
                    ValueExpr::PlayerStat(PlayerStatExpr::OpponentMinLife)
                }
                _ => {
                    let rp = lower_resolving_player(*player, builder)?;
                    if rp == ResolvingPlayer::Controller {
                        ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife)
                    } else {
                        ValueExpr::PlayerStat(PlayerStatExpr::PlayerLife(rp))
                    }
                }
            };
            Ok(ResolutionCondition {
                left,
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::ControlsCommander => Ok(ResolutionCondition {
            left: ValueExpr::count_player_battlefield(
                ResolvingPlayer::Controller,
                ObjectFilter::Clause(ObjectClause::any().commander()),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::CounterCompare {
            subject,
            counter_kind,
            op,
            value,
        } => {
            let kind = if counter_kind.eq_ignore_ascii_case("any") {
                None
            } else {
                Some(parse_counter_kind(counter_kind).ok_or("oracle:unsupported-if-condition")?)
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => {
                    if in_trigger {
                        spellwake_engine::program::CounterSubjectRef::TriggerSource
                    } else {
                        spellwake_engine::program::CounterSubjectRef::AbilitySource
                    }
                }
                SubjectRefAst::AttachedHost(_) => {
                    spellwake_engine::program::CounterSubjectRef::AttachedHost
                }
                SubjectRefAst::TriggerSubject if in_trigger => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSubject
                }
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    spellwake_engine::program::CounterSubjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_) if in_trigger => {
                    spellwake_engine::program::CounterSubjectRef::TriggerSubject
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::counters(subj, kind),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::TotalStatCompare {
            filter,
            stat,
            op,
            value,
        } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::aggregate(obj, st, StatAggregate::Sum),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::StatCompare {
            subject,
            stat,
            op,
            value,
        } => {
            let st = match stat {
                StatKind::Power => ObjectStatKind::Power,
                StatKind::Toughness => ObjectStatKind::Toughness,
                StatKind::ManaValue => ObjectStatKind::ManaValue,
                StatKind::Loyalty => ObjectStatKind::Loyalty,
            };
            let is_object_target = !matches!(
                builder.target,
                BodyTarget::None
                    | BodyTarget::SinglePlayer
                    | BodyTarget::SingleOpponent
                    | BodyTarget::SingleAnyTarget
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker
                    | BodyTarget::SingleCreatureOrPlayer
            );
            let is_self_anaphora = |a: &str| {
                matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "this creature"
                        | "this permanent"
                        | "this artifact"
                        | "this enchantment"
                        | "this land"
                        | "this planeswalker"
                )
            };
            let subj = match subject.as_ref() {
                SubjectRefAst::SelfObject => {
                    if in_trigger {
                        ValueObjectRef::TriggerSource
                    } else {
                        ValueObjectRef::AbilitySource
                    }
                }
                SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => {
                    if in_trigger {
                        ValueObjectRef::TriggerSource
                    } else {
                        ValueObjectRef::AbilitySource
                    }
                }
                SubjectRefAst::AttachedHost(_) => ValueObjectRef::AttachedHost,
                SubjectRefAst::TriggerSubject if in_trigger => ValueObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(_) if is_object_target => ValueObjectRef::TargetObject,
                SubjectRefAst::Anaphora(_) if in_trigger => ValueObjectRef::TriggerSubject,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_stat(subj, st),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::MatchesFilter { subject, filter } => {
            let is_object_target = !matches!(
                builder.target,
                BodyTarget::None | BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
            );
            let is_self_anaphora = |a: &str| {
                matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "this creature"
                        | "this permanent"
                        | "this artifact"
                        | "this enchantment"
                        | "this land"
                        | "this planeswalker"
                )
            };
            let prev_anaphora_ref = builder.instructions.iter().rev().find_map(|op| match op {
                Instruction::LookTopChoose {
                    reveal_only: true, ..
                }
                | Instruction::MayRevealLibraryTop
                | Instruction::RevealUntilMatch { .. } => Some(MatchObjectRef::RevealedTop),
                Instruction::Discard { .. } | Instruction::DiscardFiltered { .. } => {
                    Some(MatchObjectRef::DiscardedCard)
                }
                Instruction::Mill { .. } => Some(MatchObjectRef::MovedLibraryCard),
                Instruction::DestroyTarget => Some(MatchObjectRef::DestructionTarget),
                Instruction::Sacrifice { .. } => Some(MatchObjectRef::SacrificedCreature),
                _ => None,
            });
            let mref = match subject.as_ref() {
                SubjectRefAst::SelfObject => MatchObjectRef::AbilitySource,
                SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => MatchObjectRef::AbilitySource,
                SubjectRefAst::AttachedHost(_) => MatchObjectRef::AttachedHost,
                SubjectRefAst::TriggerSubject if in_trigger => MatchObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(_) if prev_anaphora_ref.is_some() && !is_object_target => {
                    prev_anaphora_ref.unwrap_or(MatchObjectRef::RevealedTop)
                }
                SubjectRefAst::Anaphora(_) if is_object_target => MatchObjectRef::TargetObject,
                SubjectRefAst::Anaphora(_) if in_trigger => MatchObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(_) if prev_anaphora_ref.is_some() => {
                    prev_anaphora_ref.unwrap_or(MatchObjectRef::RevealedTop)
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::HasStatus { subject, status } => {
            let status_low = status.trim().to_ascii_lowercase();
            let is_self_anaphora = |a: &str| {
                matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "this creature"
                        | "this permanent"
                        | "this artifact"
                        | "this enchantment"
                        | "this land"
                        | "this planeswalker"
                )
            };
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if is_self_anaphora(a))
            {
                match status_low.as_str() {
                    "saddled" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Saddled),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "renowned" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Renowned),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "paired" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Paired),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "monstrous" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Monstrous),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "suspected" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Suspected),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "solved" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Solved),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "prepared" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Prepared),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "goaded" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::Goaded),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    "exerted" => {
                        return Ok(ResolutionCondition {
                            left: ValueExpr::SourceState(SourceStateExpr::ExertedThisTurn),
                            comparison: Comparison::GreaterOrEqual,
                            right: ValueExpr::literal(1),
                        });
                    }
                    _ => {}
                }
            }
            let is_object_target = !matches!(
                builder.target,
                BodyTarget::None
                    | BodyTarget::SinglePlayer
                    | BodyTarget::SingleOpponent
                    | BodyTarget::SingleAnyTarget
                    | BodyTarget::SinglePlayerOrPlaneswalker
                    | BodyTarget::SingleOpponentOrPlaneswalker
                    | BodyTarget::SingleCreatureOrPlayer
            );
            let mref = match subject.as_ref() {
                SubjectRefAst::SelfObject => MatchObjectRef::AbilitySource,
                SubjectRefAst::Anaphora(a) if is_self_anaphora(a) => MatchObjectRef::AbilitySource,
                SubjectRefAst::AttachedHost(_) => MatchObjectRef::AttachedHost,
                SubjectRefAst::TriggerSubject if in_trigger => MatchObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(_) if is_object_target => MatchObjectRef::TargetObject,
                SubjectRefAst::Anaphora(_) if in_trigger => MatchObjectRef::TriggerSubject,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let obj = match status_low.as_str() {
                "tapped" => ObjectFilter::any().tapped(),
                "untapped" => ObjectFilter::Clause(ObjectClause::any().untapped()),
                "attacking" => ObjectFilter::any().attacking(),
                "blocking" => ObjectFilter::any().combat(CombatFilter::Blocking),
                "attacking or blocking" => ObjectFilter::any().attacking_or_blocking(),
                "blocked" => ObjectFilter::any().combat(CombatFilter::Blocked),
                "unblocked" => ObjectFilter::any().combat(CombatFilter::Unblocked),
                "enchanted" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::ENCHANTED);
                    ObjectFilter::Clause(c)
                }
                "equipped" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::EQUIPPED);
                    ObjectFilter::Clause(c)
                }
                "modified" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::MODIFIED);
                    ObjectFilter::Clause(c)
                }
                "face down" | "face-down" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::FACE_DOWN);
                    ObjectFilter::Clause(c)
                }
                "token" => {
                    let mut c = ObjectClause::any();
                    c.token = Some(true);
                    ObjectFilter::Clause(c)
                }
                "nontoken" => {
                    let mut c = ObjectClause::any();
                    c.token = Some(false);
                    ObjectFilter::Clause(c)
                }
                "legendary" => ObjectFilter::any().legendary(),
                "commander" => ObjectFilter::Clause(ObjectClause::any().commander()),
                "historic" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::HISTORIC);
                    ObjectFilter::Clause(c)
                }
                "snow" => ObjectFilter::Clause(ObjectClause {
                    required_supertypes: SupertypeMask::SNOW,
                    ..ObjectClause::any()
                }),
                "basic" => ObjectFilter::Clause(ObjectClause {
                    required_supertypes: SupertypeMask::BASIC,
                    ..ObjectClause::any()
                }),
                "colorless" => ObjectFilter::any().with_color(ObjectColorFilter::Colorless),
                "multicolored" => ObjectFilter::any().with_color(ObjectColorFilter::Multicolored),
                "monocolored" => ObjectFilter::any().with_color(ObjectColorFilter::Monocolored),
                "white" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::White)),
                "blue" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Blue)),
                "black" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Black)),
                "red" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Red)),
                "green" => ObjectFilter::any().with_color(ObjectColorFilter::has(Color::Green)),
                "creature" => ObjectFilter::creature(),
                "artifact" => ObjectFilter::artifact(),
                "enchantment" => ObjectFilter::enchantment(),
                "land" => ObjectFilter::land(),
                "planeswalker" => ObjectFilter::planeswalker(),
                "nonland" => ObjectFilter::nonland(),
                other => {
                    if let Some(sub_raw) = other.strip_prefix("is:") {
                        let sub = sub_raw.trim();
                        match sub {
                            "creature" => ObjectFilter::creature(),
                            "artifact" => ObjectFilter::artifact(),
                            "enchantment" => ObjectFilter::enchantment(),
                            "land" => ObjectFilter::land(),
                            "planeswalker" => ObjectFilter::planeswalker(),
                            "legendary" => ObjectFilter::any().legendary(),
                            _ => {
                                let title = sub
                                    .split('-')
                                    .map(|seg| {
                                        let mut ch = seg.chars();
                                        ch.next()
                                            .map(|c| {
                                                c.to_uppercase().collect::<String>() + ch.as_str()
                                            })
                                            .unwrap_or_default()
                                    })
                                    .collect::<Vec<_>>()
                                    .join("-");
                                if let Some(ct) = vocabulary.resolve_label(&title) {
                                    ObjectFilter::any().with_subtype(ct)
                                } else {
                                    let compiler_vocab = cached_compiler_vocab(vocabulary);
                                    let parsed =
                                        spellwake_oracle_compiler::filter::parse_object_filter(
                                            sub,
                                            &compiler_vocab,
                                        )
                                        .map_err(|_| "oracle:unsupported-if-condition")?;
                                    lower_object_filter(&parsed, vocabulary)
                                        .map_err(|_| "oracle:unsupported-if-condition")?
                                }
                            }
                        }
                    } else {
                        return Err("oracle:unsupported-if-condition");
                    }
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::HasKeyword { subject, keyword } => {
            let mref = match subject.as_ref() {
                SubjectRefAst::SelfObject => MatchObjectRef::AbilitySource,
                SubjectRefAst::AttachedHost(_) => MatchObjectRef::AttachedHost,
                SubjectRefAst::TriggerSubject if in_trigger => MatchObjectRef::TriggerSubject,
                SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
                    MatchObjectRef::TargetObject
                }
                SubjectRefAst::Anaphora(_) if in_trigger => MatchObjectRef::TriggerSubject,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            let kw =
                lower_keyword_ability_name(keyword).ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(mref, ObjectFilter::any().with_keyword(kw)),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CitysBlessing) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerHasCityBlessing),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Morbid {
            your_control_only: false,
        } => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Morbid {
            your_control_only: true,
        } => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControlledCreatureDiedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Revolt => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::PermanentLeftBattlefieldThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::EnteredThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::IsNotToken => {
            let mut c = ObjectClause::any();
            c.token = Some(false);
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    MatchObjectRef::AbilitySource,
                    ObjectFilter::Clause(c),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::OpponentCountCompare { op, value } => Ok(ResolutionCondition {
            left: ValueExpr::OpponentsCount,
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::DevotionCompare { colors, op, value } => {
            let mask =
                parse_devotion_color_mask(colors).ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::Devotion(mask),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::OpponentMoreThanYou { metric, filter } => {
            if let Some(f) = filter {
                let obj = lower_object_filter(f, vocabulary)?;
                return Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(obj),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                });
            }
            match metric.to_ascii_lowercase().as_str() {
                "life" => Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentMaxLife),
                    comparison: Comparison::Greater,
                    right: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLife),
                }),
                "cards" | "cards in hand" | "cards_in_hand" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Hand(ResolvingPlayer::Controller),
                        filter: Box::new(ObjectFilter::any()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_hand(ResolvingPlayer::Controller),
                }),
                "cards in graveyard" | "cards_in_graveyard" | "cards in their graveyard" => {
                    Ok(ResolutionCondition {
                        left: ValueExpr::OpponentMaxCount {
                            zone: ValueZone::Graveyard(ResolvingPlayer::Controller),
                            filter: Box::new(ObjectFilter::any()),
                        },
                        comparison: Comparison::Greater,
                        right: ValueExpr::count_graveyard(
                            ResolvingPlayer::Controller,
                            ObjectFilter::any(),
                        ),
                    })
                }
                "lands" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::land()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::land(),
                    ),
                }),
                "creatures" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::creature()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::creature(),
                    ),
                }),
                "artifacts" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::artifact()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::artifact(),
                    ),
                }),
                "enchantments" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::enchantment()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::enchantment(),
                    ),
                }),
                "permanents" | "controls" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::any()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::any(),
                    ),
                }),
                "nonland permanents" => Ok(ResolutionCondition {
                    left: ValueExpr::OpponentMaxCount {
                        zone: ValueZone::Battlefield,
                        filter: Box::new(ObjectFilter::nonland()),
                    },
                    comparison: Comparison::Greater,
                    right: ValueExpr::count_player_battlefield(
                        ResolvingPlayer::Controller,
                        ObjectFilter::nonland(),
                    ),
                }),
                _ => Err("oracle:unsupported-if-condition"),
            }
        }
        ConditionAst::WasKicked => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::KickerCount(0)),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Bargained) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::Bargained),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::DescendedThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DescendedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CommittedCrimeThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CommittedCrimeThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NoCreaturesOnBattlefield) => Ok(ResolutionCondition {
            left: ValueExpr::count_battlefield(ObjectFilter::creature()),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
            from_hand_only: false,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::DrewCardsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CardsDrawnThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::CastFromGraveyard) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromGraveyard),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastFromExile) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromExile),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::CastFromOutsideHand) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastFromOutsideHand),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::GiftPromised) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::GiftPromised),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::EvidenceCollected) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::EvidenceCollected),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ManaSpentLessThanManaValue) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::ManaSpentToCast),
            comparison: Comparison::Less,
            right: ValueExpr::ObjectStat {
                subject: ValueObjectRef::AbilitySource,
                stat: ObjectStatKind::ManaValue,
            },
        }),
        ConditionAst::ManaSpentToCast(raw) => {
            let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
            if low == "no mana" || low == "0" || low == "zero mana" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaSpentToCast),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if low == "{w}" || low == "white mana" || low == "w" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(ManaType::White)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low == "{u}" || low == "blue mana" || low == "u" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(ManaType::Blue)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low == "{b}" || low == "black mana" || low == "b" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(ManaType::Black)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low == "{r}" || low == "red mana" || low == "r" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(ManaType::Red)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low == "{g}" || low == "green mana" || low == "g" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(ManaType::Green)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low == "{c}" || low == "colorless mana" || low == "c" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(
                        ManaType::Colourless,
                    )),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low == "{s}" || low == "snow mana" || low == "snow" || low.contains("snow mana") {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::SnowManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low.contains("treasure") {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::TreasureManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if low.contains("produced by a creature") || low == "creature mana" {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::CreatureManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" colors of mana"))
                .or_else(|| low.strip_suffix(" or more colors of mana"))
                && let Some(n) = match rest.trim() {
                    "one" | "1" => Some(1i64),
                    "two" | "2" => Some(2),
                    "three" | "3" => Some(3),
                    "four" | "4" => Some(4),
                    "five" | "5" => Some(5),
                    other => other.parse::<i64>().ok().filter(|&v| v > 0),
                }
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ColorsSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if let Some(rest) = low
                .strip_prefix("at least ")
                .and_then(|s| s.strip_suffix(" mana"))
                .or_else(|| low.strip_suffix(" or more mana"))
                && let Some(n) = match rest.trim() {
                    "one" | "1" => Some(1i64),
                    "two" | "2" => Some(2),
                    "three" | "3" => Some(3),
                    "four" | "4" => Some(4),
                    "five" | "5" => Some(5),
                    "six" | "6" => Some(6),
                    "seven" | "7" => Some(7),
                    "eight" | "8" => Some(8),
                    "nine" | "9" => Some(9),
                    "ten" | "10" => Some(10),
                    other => other.parse::<i64>().ok().filter(|&v| v > 0),
                }
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaSpentToCast),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                });
            }
            if let Some((mc, 0, hybrids)) = parse_mana_cost_full(&low)
                && hybrids.is_empty()
                && mc.generic == 0.into()
                && mc.required.len() == 1
                && let Some((&mt, amt)) = mc.required.iter().next()
                && amt.is_positive()
            {
                return Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::ManaTypeSpentToCast(mt)),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::Literal(amt.clone()),
                });
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(GameMarker::CastDuringMainPhase) => Ok(ResolutionCondition {
            left: ValueExpr::event(EventValueKind::CastDuringOwnMain),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Monarch(PlayerRefAst::You)) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Initiative(PlayerRefAst::You)) => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::HasInitiative),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CompletedDungeon) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CompletedDungeons),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ControlsGreatestStat {
            player: PlayerRefAst::You,
            stat: StatKind::Power,
            ..
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControlsGreatestCreature),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::OpponentLostLifeThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::GainedLifeThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::You,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::LostLifeThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            min_amount,
        } if *min_amount > 0 => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::AttackedThisTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count }
            if *min_count > 0
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.stat_bounds.is_empty()
                && filter.token.is_none()
                && filter.flags.is_empty() =>
        {
            if filter.any_types.is_empty() {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::EnteredPermanentThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            let mut types = card_type_mask_to_set(filter.any_types)?;
            if types.len() == 1
                && let Some(kind) = types.pop_first()
            {
                let stat = if kind == CardType::Creature {
                    PlayerStatExpr::CreatureEnteredUnderControlThisTurn
                } else {
                    PlayerStatExpr::EnteredTypeThisTurn(kind)
                };
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(stat),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(i64::from(*min_count)),
                });
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::CastSpellThisTurn {
            player: PlayerRefAst::You,
            filter,
        } if filter.subtypes.is_empty()
            && filter.any_colors.is_empty()
            && filter.forbidden_colors.is_empty()
            && filter.color_arity.is_none()
            && filter.stat_bounds.is_empty()
            && filter.token.is_none()
            && filter.flags.is_empty() =>
        {
            if filter.any_types.is_empty() && filter.forbidden_types.is_empty() {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if filter.any_types.is_empty() && filter.forbidden_types == AstCardTypeMask::CREATURE {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CastNoncreatureSpellsThisTurn),
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(1),
                });
            }
            if filter.forbidden_types.is_empty() {
                if filter.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                            )),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                let mut types = card_type_mask_to_set(filter.any_types)?;
                if types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellTypeThisTurn(kind)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::IsYourTurn => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsActivePlayer),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Delirium => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OwnGraveyardDistinctCardTypes),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(4),
        }),
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
            action,
        } => {
            use spellwake_engine::program::OpponentTurnStat;
            let act_low = action.trim().to_ascii_lowercase();
            let stat = match act_low.as_str() {
                "lost_life" | "lose_life" | "lost life" | "an opponent lost life this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "discard"
                | "discarded"
                | "discarded_card"
                | "discarded a card"
                | "an opponent discarded a card this turn" => OpponentTurnStat::Discarded,
                "sacrifice"
                | "sacrificed"
                | "sacrificed_permanent"
                | "sacrificed a permanent"
                | "an opponent sacrificed a permanent this turn" => OpponentTurnStat::Sacrificed,
                "cast_spell" | "cast a spell" | "an opponent cast a spell this turn" => {
                    OpponentTurnStat::CastSpells
                }
                "drew_card" | "drew a card" | "an opponent drew a card this turn" => {
                    OpponentTurnStat::DrewCards
                }
                "attacked" | "attack" | "an opponent attacked this turn" => {
                    OpponentTurnStat::Attacked
                }
                "gained_life"
                | "gain_life"
                | "gained life"
                | "an opponent gained life this turn" => OpponentTurnStat::GainedLife,
                "played_land" | "played a land" | "an opponent played a land this turn" => {
                    OpponentTurnStat::PlayedLand
                }
                "creature_entered" | "a creature entered under an opponent's control this turn" => {
                    OpponentTurnStat::CreatureEntered
                }
                "permanent_left_battlefield" | "revolt" => OpponentTurnStat::PermanentLeft,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentStatThisTurn(stat)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::PlayerActionThisTurn {
            player: PlayerRefAst::You,
            action,
        } => {
            let act_low = action.trim().to_ascii_lowercase();
            let (stat, min): (PlayerStatExpr, i64) = match act_low.as_str() {
                "lost_life" | "lose_life" | "you lost life this turn" => {
                    (PlayerStatExpr::ControllerLifeLostThisTurn, 1)
                }
                "this is the first or second time this ability has resolved this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
                        comparison: Comparison::LessOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you didn't lose life this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "gained_life" | "gain_life" | "you gained life this turn" => {
                    (PlayerStatExpr::ControllerLifeGainedThisTurn, 1)
                }
                "attacked" | "attack" | "you attacked this turn" => {
                    (PlayerStatExpr::ControllerAttackedThisTurn, 1)
                }
                "discard" | "discarded" | "discarded_card" | "you discarded a card this turn" => {
                    (PlayerStatExpr::ControllerDiscardedThisTurn, 1)
                }
                "sacrifice"
                | "sacrificed"
                | "sacrificed_permanent"
                | "you sacrificed a permanent this turn" => {
                    (PlayerStatExpr::ControllerSacrificedThisTurn, 1)
                }
                "revolt"
                | "permanent_left_battlefield"
                | "a permanent you controlled left the battlefield this turn" => {
                    (PlayerStatExpr::PermanentLeftBattlefieldThisTurn, 1)
                }
                "descended" | "you descended this turn" => (PlayerStatExpr::DescendedThisTurn, 1),
                "crime" | "committed_crime" | "you committed a crime this turn" => {
                    (PlayerStatExpr::CommittedCrimeThisTurn, 1)
                }
                "cast_two_spells" | "you've cast two or more spells this turn" => {
                    (PlayerStatExpr::ControllerSpellsCastThisTurn, 2)
                }
                "drew_card" | "you drew a card this turn" => {
                    (PlayerStatExpr::CardsDrawnThisTurn, 1)
                }
                "drew_two_cards"
                | "drew_2plus"
                | "you've drawn two or more cards this turn"
                | "you've drawn more than one card this turn" => {
                    (PlayerStatExpr::CardsDrawnThisTurn, 2)
                }
                "you cast a spell this turn" | "you've cast a spell this turn" => {
                    (PlayerStatExpr::ControllerSpellsCastThisTurn, 1)
                }
                "an opponent lost life this turn" => (PlayerStatExpr::OpponentsLifeLostThisTurn, 1),
                "an opponent is poisoned" => (PlayerStatExpr::AnyOpponentPoisoned, 1),
                "an opponent has been dealt noncombat damage this turn"
                | "an opponent was dealt noncombat damage this turn" => (
                    PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: true,
                    },
                    1,
                ),
                "an opponent has been dealt damage this turn"
                | "an opponent was dealt damage this turn" => (
                    PlayerStatExpr::OpponentDealtDamageThisTurn {
                        noncombat_only: false,
                    },
                    1,
                ),
                "a creature entered the battlefield under your control this turn"
                | "a creature entered under your control this turn" => {
                    (PlayerStatExpr::CreatureEnteredUnderControlThisTurn, 1)
                }
                "a creature died this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "no creatures died this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CreaturesDiedThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you played a land this turn"
                | "you've played a land this turn"
                | "played_land" => (PlayerStatExpr::PlayedLandThisTurn, 1),
                "you didn't play a land this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayedLandThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you have a card in hand" | "you have at least one card in hand" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::count_hand(ResolvingPlayer::Controller),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "your library has no cards in it" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::PlayerLibrarySize(
                            ResolvingPlayer::Controller,
                        )),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "there are five colors among permanents you control" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::DistinctColors {
                            filter: Box::new(ObjectFilter::permanent().controlled()),
                        },
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(5),
                    });
                }
                "~ attacked this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::SourceAttackedThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you cast a noncreature spell this turn"
                | "you've cast a noncreature spell this turn" => {
                    (PlayerStatExpr::CastNoncreatureSpellsThisTurn, 1)
                }
                "you cast a creature spell this turn"
                | "you've cast a creature spell this turn" => {
                    (PlayerStatExpr::CastSpellTypeThisTurn(CardType::Creature), 1)
                }
                "you cast an instant or sorcery spell this turn"
                | "you've cast an instant or sorcery spell this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                            )),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "a card left your graveyard this turn"
                | "one or more cards left your graveyard this turn" => {
                    (PlayerStatExpr::CardsLeftGraveyardThisTurn, 1)
                }
                "you created a token this turn" | "you've created a token this turn" => {
                    (PlayerStatExpr::CreatedTokensThisTurn, 1)
                }
                "you gained or lost life this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::ControllerLifeGainedThisTurn,
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::ControllerLifeLostThisTurn,
                            )),
                        ),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you didn't attack this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you didn't cast a spell this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you sacrificed one or more permanents this turn" => {
                    (PlayerStatExpr::ControllerSacrificedThisTurn, 1)
                }
                "you're the defending player" => (PlayerStatExpr::ControllerIsDefending, 1),
                "~ attacked or blocked this turn" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisTurn),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "a library has twenty or fewer cards in it"
                | "a library has 20 or fewer cards in it" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::MinLibrarySize),
                        comparison: Comparison::LessOrEqual,
                        right: ValueExpr::literal(20),
                    });
                }
                "you've cast both a creature spell and a noncreature spell this turn" => {
                    let c1 = ValueExpr::Conditional {
                        left: Box::new(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastSpellTypeThisTurn(CardType::Creature),
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(ValueExpr::literal(1)),
                        then_value: Box::new(ValueExpr::literal(1)),
                        else_value: Box::new(ValueExpr::literal(0)),
                    };
                    let c2 = ValueExpr::Conditional {
                        left: Box::new(ValueExpr::PlayerStat(
                            PlayerStatExpr::CastNoncreatureSpellsThisTurn,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: Box::new(ValueExpr::literal(1)),
                        then_value: Box::new(ValueExpr::literal(1)),
                        else_value: Box::new(ValueExpr::literal(0)),
                    };
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(Box::new(c1), Box::new(c2)),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(2),
                    });
                }
                "an opponent discarded a card this turn" => (
                    PlayerStatExpr::OpponentStatThisTurn(
                        spellwake_engine::program::OpponentTurnStat::Discarded,
                    ),
                    1,
                ),
                "it's your main phase" | "your main phase" => (PlayerStatExpr::IsOwnMainPhase, 1),
                "you've activated a loyalty ability this turn"
                | "you activated a loyalty ability this turn" => {
                    (PlayerStatExpr::LoyaltyActivatedThisTurn, 1)
                }
                "you've completed a dungeon" | "completed a dungeon" => {
                    (PlayerStatExpr::CompletedDungeons, 1)
                }
                "you have the initiative" | "have the initiative" => {
                    (PlayerStatExpr::HasInitiative, 1)
                }
                "the ring has tempted you" => (PlayerStatExpr::RingLevel, 1),
                "you won the flip" | "won the flip" | "you win the flip" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you lose the flip" | "you lost the flip" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastCoinFlipWon),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "you won the clash" | "won the clash" | "you win" | "you do" | "they do"
                | "that player does" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "you don't" | "they don't" | "that player doesn't" | "you can't" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                "~ is still on the battlefield" | "it's still on the battlefield" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::SourceState(SourceStateExpr::InZone(
                            spellwake_engine::program::SourceZoneKind::Battlefield,
                        )),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "it was cast from your graveyard" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::CastFromGraveyard),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                "~ was cast using web-slinging" | "that creature was cast for its warp cost" => {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::PaidAlternative),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    });
                }
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(stat),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(min),
            })
        }
        ConditionAst::Marker(GameMarker::InZone { subject, zone })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                || matches!(subject.as_ref(), SubjectRefAst::Anaphora(a) if a.eq_ignore_ascii_case("it") || a.eq_ignore_ascii_case("this card")) =>
        {
            use spellwake_engine::program::SourceZoneKind;
            let sz = match zone {
                ZoneFilter::Battlefield => SourceZoneKind::Battlefield,
                ZoneFilter::YourGraveyard | ZoneFilter::AnyGraveyard => SourceZoneKind::Graveyard,
                ZoneFilter::YourHand => SourceZoneKind::Hand,
                ZoneFilter::Exile => SourceZoneKind::Exile,
                ZoneFilter::Stack => SourceZoneKind::Stack,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(sz)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::KeywordCostPaid(kw)) => {
            let kind = match kw.trim().to_ascii_lowercase().as_str() {
                "kicker" | "kicked" | "multikicker" | "offspring" | "squad" | "entwine"
                | "buyback" => EventValueKind::KickerCount(0),
                "bargain" | "bargained" => EventValueKind::Bargained,
                "cleave" | "cleaved" => EventValueKind::Cleaved,
                "gift" | "gifted" => EventValueKind::GiftPromised,
                "overload" | "surge" | "spectacle" | "prowl" | "dash" | "blitz" | "evoke"
                | "escape" | "madness" | "miracle" | "foretell" | "plot" | "emerge"
                | "freerunning" | "warp" | "web-slinging" | "bestow" | "disturb" | "mutate"
                | "flashback" | "prototype" | "mayhem" | "sneak" | "harmonize" | "encore"
                | "unearth" | "ninjutsu" | "suspend" => EventValueKind::PaidAlternative,
                _ => return Err("oracle:unsupported-if-condition"),
            };
            Ok(ResolutionCondition {
                left: ValueExpr::event(kind),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::Monarch(
            PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
        )) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentIsMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NoMonarch) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::NoMonarch),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::MaxSpeed) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpeed),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(4),
        }),
        ConditionAst::Marker(GameMarker::FullParty) => {
            let roles = resolve_party_roles(vocabulary).ok_or("oracle:unsupported-if-condition")?;
            Ok(ResolutionCondition {
                left: ValueExpr::PartyCount(Box::new(roles)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(4),
            })
        }
        ConditionAst::Marker(GameMarker::YouAreAttacking) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsAttacking),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::YouAreDefending) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerIsDefending),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::RemainsTapped) => Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                MatchObjectRef::AbilitySource,
                ObjectFilter::any().tapped(),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackingAlone) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AttackingAlone),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedOrBlockedThisCombat) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisCombat),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedThisCombat { subject })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(
                    MatchObjectRef::AbilitySource,
                    ObjectFilter::any().attacking(),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AttackedThisTurn { subject })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AttackedOrBlockedThisTurn { subject })
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AttackedOrBlockedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
            subject,
            or_entered_this_turn: false,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotAttackThisTurn {
            subject,
            or_entered_this_turn: true,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn)),
                Box::new(ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn)),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::SubjectEnteredThisTurn(subject))
            if matches!(subject.as_ref(), SubjectRefAst::SelfObject) =>
        {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::EnteredThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::DamageDealtToSubjectThisTurn {
            subject,
            min_amount,
        }) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => Ok(ResolutionCondition {
            left: ValueExpr::SourceState(SourceStateExpr::DamageDealtThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_amount)),
        }),
        ConditionAst::Marker(GameMarker::OpponentDealtDamageThisTurn { noncombat_only }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentDealtDamageThisTurn {
                    noncombat_only: *noncombat_only,
                }),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::CardLeftGraveyardThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CardsLeftGraveyardThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::NonlandPermanentLeftThisTurn) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::NonlandPermanentLeftBattlefieldThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::counters(
                spellwake_engine::program::CounterSubjectRef::ControllerPoison,
                Some(CounterKind::Poison),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerIsPoisoned {
            player: PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AnyOpponentPoisoned),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::AttackedWithMinCreaturesThisTurn {
            player: PlayerRefAst::You,
            min_count,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::AttackingCreaturesThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(i64::from(*min_count)),
        }),
        ConditionAst::Marker(GameMarker::CastSpellsThisTurn {
            player: PlayerRefAst::You,
            op,
            value,
            from_hand_only: true,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellsFromHandThisTurn),
            comparison: lower_comparison(*op),
            right: ValueExpr::literal(i64::from(*value)),
        }),
        ConditionAst::Marker(GameMarker::DifferentManaValuesInGraveyard { op, value }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::DistinctManaValuesInGraveyard(ResolvingPlayer::Controller),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Marker(GameMarker::PlayerGainedOrLostLifeThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::Add(
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerLifeGainedThisTurn,
                )),
                Box::new(ValueExpr::PlayerStat(
                    PlayerStatExpr::ControllerLifeLostThisTurn,
                )),
            ),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::ControlsMoreThanOpponents {
            filter,
            than_each_other,
        }) => {
            let obj = lower_object_filter(filter, vocabulary)?;
            let rhs = if *than_each_other {
                ValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                }
            } else {
                ValueExpr::OpponentMinCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                }
            };
            Ok(ResolutionCondition {
                left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                comparison: Comparison::Greater,
                right: rhs,
            })
        }
        ConditionAst::Marker(GameMarker::ControlsFewerThanOpponent { filter }) => {
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_player_battlefield(ResolvingPlayer::Controller, obj),
                comparison: Comparison::Less,
                right: ValueExpr::OpponentMaxCount {
                    zone: ValueZone::Battlefield,
                    filter: Box::new(obj),
                },
            })
        }
        ConditionAst::Marker(GameMarker::HandMoreThanEachOpponent) => Ok(ResolutionCondition {
            left: ValueExpr::count_hand(ResolvingPlayer::Controller),
            comparison: Comparison::Greater,
            right: ValueExpr::OpponentMaxCount {
                zone: ValueZone::Hand(ResolvingPlayer::Controller),
                filter: Box::new(ObjectFilter::any()),
            },
        }),
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player: PlayerRefAst::You,
            filter: None,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::DidNotCastThisTurn {
            player: PlayerRefAst::You,
            filter: Some(f),
        }) if f.subtypes.is_empty()
            && f.any_colors.is_empty()
            && f.forbidden_colors.is_empty()
            && f.color_arity.is_none()
            && f.stat_bounds.is_empty()
            && f.token.is_none()
            && f.flags.is_empty() =>
        {
            if f.any_types.is_empty() && f.forbidden_types == AstCardTypeMask::CREATURE {
                return Ok(ResolutionCondition {
                    left: ValueExpr::PlayerStat(PlayerStatExpr::CastNoncreatureSpellsThisTurn),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                });
            }
            if f.forbidden_types.is_empty() {
                if f.any_types == AstCardTypeMask::INSTANT.union(AstCardTypeMask::SORCERY) {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::Add(
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Instant),
                            )),
                            Box::new(ValueExpr::PlayerStat(
                                PlayerStatExpr::CastSpellTypeThisTurn(CardType::Sorcery),
                            )),
                        ),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
                let mut types = card_type_mask_to_set(f.any_types)?;
                if types.len() == 1
                    && let Some(kind) = types.pop_first()
                {
                    return Ok(ResolutionCondition {
                        left: ValueExpr::PlayerStat(PlayerStatExpr::CastSpellTypeThisTurn(kind)),
                        comparison: Comparison::Equal,
                        right: ValueExpr::literal(0),
                    });
                }
            }
            Err("oracle:unsupported-if-condition")
        }
        ConditionAst::Marker(GameMarker::DidNotAttackWithCreatureThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::CreatedTokenThisTurn {
            player: PlayerRefAst::You,
        }) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::CreatedTokensThisTurn),
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::PlayerHasMoreHandThan { player, than }) => {
            let rp_a = lower_resolving_player(*player, builder)?;
            let rp_b = lower_resolving_player(*than, builder)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_hand(rp_a),
                comparison: Comparison::Greater,
                right: ValueExpr::count_hand(rp_b),
            })
        }
        ConditionAst::Marker(GameMarker::Day) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::Night) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(2),
        }),
        ConditionAst::Marker(GameMarker::NeitherDayNorNight) => Ok(ResolutionCondition {
            left: ValueExpr::PlayerStat(PlayerStatExpr::DayNightState),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(0),
        }),
        ConditionAst::Marker(GameMarker::ControlsSelf) => Ok(ResolutionCondition {
            left: ValueExpr::object_matches(
                MatchObjectRef::AbilitySource,
                ObjectFilter::permanent().controlled(),
            ),
            comparison: Comparison::Equal,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::Marker(GameMarker::RemainsOnBattlefield) if in_trigger => {
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::TriggerSourceOnBattlefield),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::AbilityActivatedThisTurn { op, value }) => {
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AbilityActivatedThisTurn),
                comparison: lower_comparison(*op),
                right: ValueExpr::literal(i64::from(*value)),
            })
        }
        ConditionAst::Marker(GameMarker::AbilityResolvedNthTimeThisTurn(n)) if *n >= 1 => {
            // `ability_resolutions_by_source_this_turn` increments when an ability finishes
            // resolving, so during its Nth resolution this turn the stored count is N - 1.
            Ok(ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AbilityResolvedCountThisTurn),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(i64::from(*n - 1)),
            })
        }
        ConditionAst::WasKickedSlot(slot_str) => {
            let slot: usize = slot_str.trim().parse().unwrap_or(0);
            Ok(ResolutionCondition {
                left: ValueExpr::event(EventValueKind::KickerCount(slot)),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::TargetsFilter(f) if builder.target != BodyTarget::None => {
            let obj = lower_object_filter(f, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::object_matches(MatchObjectRef::TargetObject, obj),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::SharesTypeWithSelf => Ok(ResolutionCondition {
            left: ValueExpr::CountOtherSharingSourceSubtype,
            comparison: Comparison::GreaterOrEqual,
            right: ValueExpr::literal(1),
        }),
        ConditionAst::WonContest => {
            let last_was_clash = builder
                .instructions
                .iter()
                .rev()
                .find_map(|op| match op {
                    Instruction::Clash { .. } => Some(true),
                    Instruction::FlipCoins { .. } | Instruction::FlipCoin { .. } => Some(false),
                    _ => None,
                })
                .unwrap_or(false);
            let kind = if last_was_clash {
                EventValueKind::LastActionSucceeded
            } else {
                EventValueKind::LastCoinFlipWon
            };
            Ok(ResolutionCondition {
                left: ValueExpr::event(kind),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            })
        }
        ConditionAst::Marker(GameMarker::VoteResult { winning }) => {
            let options = builder
                .instructions
                .iter()
                .rev()
                .find_map(|op| match op {
                    Instruction::Vote { options, .. } => Some(options),
                    _ => None,
                })
                .ok_or("oracle:unsupported-if-condition")?;
            let w_low = winning.trim().to_ascii_lowercase();
            let includes_tie = w_low.contains("tied") || w_low.contains("tie");
            let needle = w_low
                .split(" gets more")
                .next()
                .unwrap_or(&w_low)
                .split(" get more")
                .next()
                .unwrap_or(&w_low)
                .split(" receives more")
                .next()
                .unwrap_or(&w_low)
                .split(" or the vote")
                .next()
                .unwrap_or(&w_low)
                .trim()
                .trim_matches('"')
                .trim();
            let idx = options
                .iter()
                .position(|opt| {
                    let o = opt.trim().to_ascii_lowercase();
                    o == needle || needle.contains(&o) || o.contains(needle)
                })
                .ok_or("oracle:unsupported-if-condition")?;
            let idx_u8 = u8::try_from(idx).map_err(|_| "oracle:unsupported-if-condition")?;
            if includes_tie {
                if options.len() == 2 {
                    let other = 1u8 - idx_u8;
                    Ok(ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::VoteCount(idx_u8)),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::event(EventValueKind::VoteCount(other)),
                    })
                } else {
                    Err("oracle:unsupported-if-condition")
                }
            } else {
                Ok(ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::WinningVoteOption),
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(i64::from(idx_u8)),
                })
            }
        }
        ConditionAst::EnteredUnderYourControlThisTurn { filter, min_count } if *min_count > 0 => {
            let mut inner = filter.as_ref().clone();
            inner.controller = Some(ControllerRelation::You);
            inner.flags.0 |= AstFilterFlags::ENTERED_THIS_TURN.0;
            let obj = lower_object_filter(&inner, vocabulary)?;
            Ok(ResolutionCondition {
                left: ValueExpr::count_battlefield(obj),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(i64::from(*min_count)),
            })
        }
        _ => Err("oracle:unsupported-if-condition"),
    }
}

pub(super) fn lower_effect_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<ExecutableBody, &'static str> {
    lower_effect_body_impl(
        ins,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        !in_trigger,
    )
}

pub(super) fn lower_trigger_effect_body(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    spec: &TriggerSpec,
    self_pronoun_valid: bool,
) -> Result<ExecutableBody, &'static str> {
    let mut builder = BodyBuilder::with_trigger_hint(TriggerHint::from_event(&spec.event));
    lower_instruction_seq(
        ins,
        &mut builder,
        vocabulary,
        ids,
        true,
        self_pronoun_valid,
        None,
    )?;
    builder.finish()
}

pub(super) fn lower_effect_body_impl(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    allow_fixed_clauses: bool,
) -> Result<ExecutableBody, &'static str> {
    let mut builder = BodyBuilder::new();
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
        Err(err @ ("oracle:multiple-targets" | "oracle:conflicting-targets"))
            if allow_fixed_clauses && !in_trigger =>
        {
            lower_multi_target_fixed_clauses(ins, vocabulary, ids, self_pronoun_valid)
                .map_err(|_| err)
        }
        Err(err) => Err(err),
    }
}

fn expand_top_level_compound_targets(ins: &[InstructionAst]) -> Vec<InstructionAst> {
    let mut out = Vec::with_capacity(ins.len());
    for item in ins {
        match item {
            InstructionAst::TapPermanent { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::TapPermanent {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::UntapPermanent { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::UntapPermanent {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::Destroy { subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::Destroy {
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::Exile { actor, subject } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::Exile {
                            actor: *actor,
                            subject: Box::new(p.clone()),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::ReturnZone {
                subject,
                from_zone,
                to,
                tapped,
                transformed,
                with_counter,
                attached_to,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::ReturnZone {
                            subject: Box::new(p.clone()),
                            from_zone: from_zone.clone(),
                            to: to.clone(),
                            tapped: *tapped,
                            transformed: *transformed,
                            with_counter: with_counter.clone(),
                            attached_to: attached_to.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::MoveZone { subject, to } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::MoveZone {
                            subject: Box::new(p.clone()),
                            to: to.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::ModifyPT {
                subject,
                dx,
                dy,
                duration,
                scale,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::ModifyPT {
                            subject: Box::new(p.clone()),
                            dx: dx.clone(),
                            dy: dy.clone(),
                            duration: duration.clone(),
                            scale: scale.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::GrantKeywords {
                subject,
                keywords,
                duration,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::GrantKeywords {
                            subject: Box::new(p.clone()),
                            keywords: keywords.clone(),
                            duration: duration.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::PutCounters {
                subject,
                counter_kind,
                count,
            } => {
                if let SubjectRefAst::Compound(parts) = subject.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::PutCounters {
                            subject: Box::new(p.clone()),
                            counter_kind: counter_kind.clone(),
                            count: count.clone(),
                        });
                    }
                    continue;
                }
            }
            InstructionAst::DealDamage {
                source,
                target,
                amount,
            } => {
                if let SubjectRefAst::Compound(parts) = target.as_ref()
                    && parts.len() >= 2
                {
                    for p in parts {
                        out.push(InstructionAst::DealDamage {
                            source: source.clone(),
                            target: Box::new(p.clone()),
                            amount: amount.clone(),
                        });
                    }
                    continue;
                }
            }
            _ => {}
        }
        out.push(item.clone());
    }
    out
}

fn is_still_a_land_restriction(ins: &InstructionAst) -> bool {
    matches!(
        ins,
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } if restriction.eq_ignore_ascii_case("it's still a land")
            || restriction.eq_ignore_ascii_case("they're still lands")
    )
}

fn contains_replace_effect(ops: &[InstructionAst]) -> bool {
    ops.iter().any(|op| match op {
        InstructionAst::ReplaceEffect(_) => true,
        InstructionAst::IfCondition { then_body, .. } => contains_replace_effect(then_body),
        _ => false,
    })
}

fn is_optional_replace_effect(ops: &[InstructionAst]) -> bool {
    matches!(ops, [InstructionAst::Optional(inner)] if contains_replace_effect(inner))
}

fn is_replace_effect_carrier(op: &InstructionAst) -> bool {
    match op {
        InstructionAst::ReplaceEffect(_) => true,
        InstructionAst::Optional(inner) => contains_replace_effect(inner),
        InstructionAst::IfCondition { then_body, .. }
        | InstructionAst::IfPreviousSucceeded(then_body)
        | InstructionAst::IfPreviousFailed(then_body) => {
            contains_replace_effect(then_body) || is_optional_replace_effect(then_body)
        }
        _ => false,
    }
}

fn strip_replace_effect(ops: &[InstructionAst]) -> Vec<InstructionAst> {
    let mut out = Vec::with_capacity(ops.len() + 2);
    for op in ops {
        match op {
            InstructionAst::ReplaceEffect(alt_inner) => {
                out.extend(strip_replace_effect(alt_inner));
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } if contains_replace_effect(then_body) => {
                out.push(InstructionAst::IfCondition {
                    condition: condition.clone(),
                    then_body: strip_replace_effect(then_body),
                });
            }
            _ => out.push(op.clone()),
        }
    }
    out
}

fn find_first_token_spec(ops: &[InstructionAst]) -> Option<&TokenSpecAst> {
    for op in ops {
        match op {
            InstructionAst::CreateToken { token, .. } => {
                if !matches!(token, TokenSpecAst::Predefined { name, .. } if name == "those_tokens")
                {
                    return Some(token);
                }
            }
            InstructionAst::IfCondition { then_body, .. }
            | InstructionAst::Optional(then_body)
            | InstructionAst::ReplaceEffect(then_body) => {
                if let Some(tok) = find_first_token_spec(then_body) {
                    return Some(tok);
                }
            }
            _ => {}
        }
    }
    None
}

fn rewrite_those_tokens_in_slice(
    ops: &[InstructionAst],
    base_tok: &TokenSpecAst,
) -> Vec<InstructionAst> {
    ops.iter()
        .map(|op| match op {
            InstructionAst::CreateToken {
                controller,
                token:
                    TokenSpecAst::Predefined {
                        name,
                        count,
                        tapped,
                    },
            } if name == "those_tokens" => {
                let mut replaced = base_tok.clone();
                match &mut replaced {
                    TokenSpecAst::Creature {
                        count: c,
                        tapped: t,
                        ..
                    }
                    | TokenSpecAst::Predefined {
                        count: c,
                        tapped: t,
                        ..
                    }
                    | TokenSpecAst::Copy {
                        count: c,
                        tapped: t,
                        ..
                    } => {
                        *c = count.clone();
                        *t |= *tapped;
                    }
                    TokenSpecAst::Role { .. } => {}
                }
                InstructionAst::CreateToken {
                    controller: *controller,
                    token: replaced,
                }
            }
            InstructionAst::IfCondition {
                condition,
                then_body,
            } => InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: rewrite_those_tokens_in_slice(then_body, base_tok),
            },
            InstructionAst::Optional(inner) => {
                InstructionAst::Optional(rewrite_those_tokens_in_slice(inner, base_tok))
            }
            InstructionAst::ReplaceEffect(inner) => {
                InstructionAst::ReplaceEffect(rewrite_those_tokens_in_slice(inner, base_tok))
            }
            other => other.clone(),
        })
        .collect()
}

fn substitute_those_tokens(
    ops: &[InstructionAst],
    base_slice: &[InstructionAst],
) -> Vec<InstructionAst> {
    if let Some(base_tok) = find_first_token_spec(base_slice) {
        rewrite_those_tokens_in_slice(ops, base_tok)
    } else {
        ops.to_vec()
    }
}

fn is_choose_new_targets_for_copy(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::MayChooseNewTargetsForCopy => true,
        InstructionAst::Optional(inner) => matches!(
            inner.as_slice(),
            [InstructionAst::ChangeTargets { target }]
                if matches!(target.as_ref(), SubjectRefAst::Anaphora(_))
        ),
        InstructionAst::AbilityRestriction {
            restriction,
            condition: None,
        } => matches!(
            restriction.trim().to_ascii_lowercase().as_str(),
            "you may choose new targets for the copy"
                | "you may choose new targets for it"
                | "you may choose new targets for the copies"
                | "choose new targets for the copy"
                | "you may choose a new target for the copy"
                | "choose a new target for the copy"
                | "you may choose new targets for that copy"
                | "you may choose a new target for that copy"
        ),
        _ => false,
    }
}

fn resolve_stack_copy_target(
    subject: &SubjectRefAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<spellwake_engine::program::StackCopyTarget, &'static str> {
    use spellwake_engine::program::StackCopyTarget;
    match subject {
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } => {
            if filter.is_ability && !filter.any_spell {
                let no_extra_predicates = filter.any_types.is_empty()
                    && filter.forbidden_types.is_empty()
                    && filter.required_supertypes.is_empty()
                    && filter.forbidden_supertypes.is_empty()
                    && filter.subtypes.is_empty()
                    && filter.forbidden_subtypes.is_empty()
                    && filter.any_colors.is_empty()
                    && filter.forbidden_colors.is_empty()
                    && filter.required_keywords.is_empty()
                    && filter.forbidden_keywords.is_empty()
                    && filter.stat_bounds.is_empty()
                    && filter.flags.is_empty()
                    && filter.named.is_none()
                    && filter.counter_kind.is_none()
                    && filter.combat.is_none()
                    && filter.tapped.is_none()
                    && filter.token.is_none();
                if no_extra_predicates {
                    match filter.controller {
                        Some(ControllerRelation::You | ControllerRelation::YourTeam)
                            if matches!(filter.color_arity, None | Some(ColorArity::Colorless)) =>
                        {
                            builder.bind_target(BodyTarget::ControlledAbility {
                                colorless_source: matches!(
                                    filter.color_arity,
                                    Some(ColorArity::Colorless)
                                ),
                            })?;
                            return Ok(StackCopyTarget::TargetAbility);
                        }
                        None if filter.color_arity.is_none() => {
                            builder.bind_target(BodyTarget::SingleStackEntry)?;
                            return Ok(StackCopyTarget::TargetAbility);
                        }
                        _ => {}
                    }
                }
                let bt = lower_target_subject(subject, vocabulary)?;
                if !matches!(
                    bt,
                    BodyTarget::ControlledAbility { .. }
                        | BodyTarget::SingleStackEntry
                        | BodyTarget::SingleStackObject(_)
                        | BodyTarget::StackEntryTargetingControlledPermanent
                ) {
                    return Err("oracle:unsupported-copy");
                }
                builder.bind_target(bt)?;
                Ok(StackCopyTarget::TargetAbility)
            } else if filter.is_ability && filter.any_spell {
                let mut inner = filter.as_ref().clone();
                inner.is_ability = false;
                inner.any_spell = false;
                inner.zone = None;
                let sf = lower_spell_filter(&inner, vocabulary)?;
                builder.bind_target(BodyTarget::SingleStackObject(sf))?;
                Ok(StackCopyTarget::TargetSpellOrAbility)
            } else if filter.any_spell || filter.zone == Some(ZoneFilter::Stack) {
                let bt = lower_target_subject(subject, vocabulary)?;
                if !matches!(
                    bt,
                    BodyTarget::SingleSpell(_)
                        | BodyTarget::SingleStackObject(_)
                        | BodyTarget::SingleStackEntry
                ) {
                    return Err("oracle:unsupported-copy");
                }
                builder.bind_target(bt)?;
                Ok(StackCopyTarget::TargetSpell)
            } else {
                Err("oracle:unsupported-copy")
            }
        }
        SubjectRefAst::Anaphora(a) => match builder.target {
            BodyTarget::SingleSpell(_) => Ok(StackCopyTarget::TargetSpell),
            BodyTarget::ControlledAbility { .. } => Ok(StackCopyTarget::TargetAbility),
            BodyTarget::SingleStackEntry
            | BodyTarget::SingleStackObject(_)
            | BodyTarget::StackEntryTargetingControlledPermanent => {
                Ok(StackCopyTarget::TargetSpellOrAbility)
            }
            BodyTarget::None if in_trigger => {
                let alow = a.trim().to_ascii_lowercase();
                if alow.contains("ability") && !alow.contains("spell") {
                    Ok(StackCopyTarget::TriggeringAbility)
                } else {
                    Ok(StackCopyTarget::TriggeringSpell)
                }
            }
            BodyTarget::None
                if !self_pronoun_valid
                    && matches!(
                        a.trim().to_ascii_lowercase().as_str(),
                        "it" | "this spell" | "~"
                    ) =>
            {
                Ok(StackCopyTarget::ResolvingSpell)
            }
            _ => Err("oracle:unsupported-copy"),
        },
        SubjectRefAst::TriggerSubject if in_trigger => Ok(StackCopyTarget::TriggeringSpell),
        SubjectRefAst::SelfObject => {
            if !in_trigger && !self_pronoun_valid {
                Ok(StackCopyTarget::ResolvingSpell)
            } else if in_trigger {
                Ok(StackCopyTarget::TriggeringSpell)
            } else {
                Err("oracle:unsupported-copy")
            }
        }
        _ => Err("oracle:unsupported-copy"),
    }
}

struct LoweredReplacePair {
    base_ops: Vec<Instruction>,
    alt_ops: Vec<Instruction>,
    merged_target: BodyTarget,
    merged_gy: Option<(usize, usize)>,
    x_target_count: bool,
    last_group_filter: Option<ObjectFilter>,
}

#[allow(clippy::too_many_arguments)]
fn lower_replace_pair(
    base_slice: &[InstructionAst],
    raw_alt: &[InstructionAst],
    builder: &BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<LoweredReplacePair, &'static str> {
    let stripped = strip_replace_effect(raw_alt);
    let alt_slice = substitute_those_tokens(&stripped, base_slice);

    let mut base_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
    base_builder.target = builder.target;
    base_builder.graveyard_target_bounds = builder.graveyard_target_bounds;
    base_builder.x_target_count = builder.x_target_count;
    base_builder.last_group_filter = builder.last_group_filter;
    lower_instruction_seq(
        base_slice,
        &mut base_builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    )?;

    let mut alt_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
    alt_builder.target = builder.target;
    alt_builder.graveyard_target_bounds = builder.graveyard_target_bounds;
    alt_builder.x_target_count = builder.x_target_count;
    alt_builder.last_group_filter = builder.last_group_filter;
    let alt_first = lower_instruction_seq(
        &alt_slice,
        &mut alt_builder,
        vocabulary,
        ids,
        in_trigger,
        self_pronoun_valid,
        bound_x,
    );
    let need_relower = match alt_first {
        Err(_) => {
            base_builder.target != BodyTarget::None || base_builder.last_group_filter.is_some()
        }
        Ok(()) => {
            alt_builder.target == BodyTarget::None
                && base_builder.target != BodyTarget::None
                && alt_slice.iter().any(instruction_references_target_anaphora)
        }
    };
    if need_relower {
        alt_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
        alt_builder.target = base_builder.target;
        alt_builder.graveyard_target_bounds = base_builder.graveyard_target_bounds;
        alt_builder.x_target_count = base_builder.x_target_count;
        alt_builder.last_group_filter =
            base_builder.last_group_filter.or(builder.last_group_filter);
        lower_instruction_seq(
            &alt_slice,
            &mut alt_builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
    } else {
        alt_first?;
    }

    // Valid if targets match, OR if one side binds a target and the other is untargeted
    // (CR 601.2c: target is still chosen on cast and consumed on the targeted path).
    let targets_compatible = alt_builder.target == base_builder.target
        || base_builder.target == BodyTarget::None
        || alt_builder.target == BodyTarget::None;
    let gy_compatible = match (
        base_builder.graveyard_target_bounds,
        alt_builder.graveyard_target_bounds,
    ) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    };
    if !targets_compatible || !gy_compatible {
        return Err("oracle:replace-effect-target-mismatch");
    }
    let merged_target = if base_builder.target != BodyTarget::None {
        base_builder.target
    } else {
        alt_builder.target
    };
    let merged_gy = base_builder
        .graveyard_target_bounds
        .or(alt_builder.graveyard_target_bounds);
    let last_group_filter = base_builder
        .last_group_filter
        .or(alt_builder.last_group_filter);

    Ok(LoweredReplacePair {
        base_ops: base_builder.instructions,
        alt_ops: alt_builder.instructions,
        merged_target,
        merged_gy,
        x_target_count: base_builder.x_target_count || alt_builder.x_target_count,
        last_group_filter,
    })
}

fn base_pair_len(ins: &[InstructionAst], i: usize) -> usize {
    if i + 1 >= ins.len() || is_replace_effect_carrier(&ins[i + 1]) {
        return 1;
    }
    let is_two_op = match (&ins[i], &ins[i + 1]) {
        (
            InstructionAst::ModifyPT {
                subject: s1,
                duration: d1,
                ..
            },
            InstructionAst::GrantKeywords {
                subject: s2,
                duration: d2,
                ..
            },
        ) => s1 == s2 && d1 == d2,
        (InstructionAst::AnimateOrSetBasePT { .. }, InstructionAst::KeepExistingTypes(_)) => true,
        (InstructionAst::AnimateOrSetBasePT { .. }, next) => is_still_a_land_restriction(next),
        (
            InstructionAst::Copy {
                exception: None, ..
            },
            next,
        ) => is_choose_new_targets_for_copy(next),
        (
            InstructionAst::ChangeLife { gain: false, .. },
            InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                gain: true,
                ..
            },
        )
        | (
            InstructionAst::SearchLibrary { shuffle: false, .. },
            InstructionAst::ShuffleLibrary { .. },
        )
        | (InstructionAst::Exile { .. }, InstructionAst::DelayedTrigger { .. })
        | (InstructionAst::ExileTopLibrary { .. }, InstructionAst::GrantCastPermission { .. }) => {
            true
        }
        _ => false,
    };
    if is_two_op { 2 } else { 1 }
}

fn logical_unit_len(ins: &[InstructionAst], i: usize) -> usize {
    let base = base_pair_len(ins, i);
    if i + base < ins.len() && is_replace_effect_carrier(&ins[i + base]) {
        if matches!(&ins[i + base], InstructionAst::IfCondition { .. })
            && i + base + 1 < ins.len()
            && matches!(&ins[i + base + 1], InstructionAst::Otherwise(_))
        {
            return base + 2;
        }
        return base + 1;
    }
    if i + 1 < ins.len() {
        match (&ins[i], &ins[i + 1]) {
            (InstructionAst::IfCondition { .. }, InstructionAst::Otherwise(_)) => return 2,
            (InstructionAst::FlipCoin, InstructionAst::IfCondition { condition, .. })
                if matches!(condition.as_ref(), ConditionAst::WonContest) =>
            {
                if i + 2 < ins.len() && matches!(&ins[i + 2], InstructionAst::Otherwise(_)) {
                    return 3;
                }
                return 2;
            }
            (
                InstructionAst::Optional(_) | InstructionAst::OptionalPlayer { .. },
                InstructionAst::IfPreviousSucceeded(_)
                | InstructionAst::IfPreviousFailed(_)
                | InstructionAst::ReflexiveTrigger(_),
            ) => {
                if i + 2 < ins.len() && matches!(&ins[i + 2], InstructionAst::Otherwise(_)) {
                    return 3;
                }
                return 2;
            }
            (
                InstructionAst::CreateToken { .. },
                InstructionAst::Attach { .. } | InstructionAst::DelayedTrigger { .. },
            ) => return 2,
            (InstructionAst::RevealHand { .. }, InstructionAst::Discard { .. }) => return 2,
            _ => {}
        }
    }
    base
}

fn lower_multi_target_fixed_clauses(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    self_pronoun_valid: bool,
) -> Result<ExecutableBody, &'static str> {
    let expanded = expand_top_level_compound_targets(ins);
    let mut standalone_x: Option<&ValueExprAst> = None;
    let mut empty_bind_count = 0usize;
    for item in &expanded {
        if let InstructionAst::BindX { x_expr, body } = item
            && body.is_empty()
        {
            standalone_x = Some(x_expr.as_ref());
            empty_bind_count += 1;
        }
    }
    let bound_x = if empty_bind_count == 1 {
        standalone_x
    } else {
        None
    };

    let mut outer_ops: Vec<Instruction> = Vec::new();
    let mut active_clause: Option<BodyBuilder> = None;
    let mut clause_count = 0usize;

    let flush_clause = |active: &mut Option<BodyBuilder>,
                        outer: &mut Vec<Instruction>,
                        count: &mut usize|
     -> Result<(), &'static str> {
        if let Some(clause) = active.take() {
            if clause.target == BodyTarget::None
                || clause.instructions.is_empty()
                || clause.graveyard_target_bounds.is_some()
            {
                return Err("oracle:invalid-fixed-clause");
            }
            let body = clause.finish()?;
            if body.permanent_target_bounds().is_some()
                || !body.instructions.iter().any(Instruction::uses_mode_target)
            {
                return Err("oracle:invalid-fixed-clause");
            }
            outer.push(Instruction::FixedTargetClause(Box::new(body)));
            *count += 1;
        }
        Ok(())
    };

    let mut i = 0usize;
    while i < expanded.len() {
        if bound_x.is_some()
            && matches!(&expanded[i], InstructionAst::BindX { body, .. } if body.is_empty())
        {
            i += 1;
            continue;
        }
        let step = logical_unit_len(&expanded, i);
        let unit = &expanded[i..i + step];
        let unit_has_anaphora = unit.iter().any(instruction_references_target_anaphora);

        if let Some(cur) = active_clause.as_mut()
            && unit_has_anaphora
        {
            let mut try_cur = cur.clone();
            if lower_instruction_seq(
                unit,
                &mut try_cur,
                vocabulary,
                ids,
                false,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && try_cur.target == cur.target
            {
                *cur = try_cur;
                i += step;
                continue;
            }
        }

        let mut probe = BodyBuilder::new();
        lower_instruction_seq(
            unit,
            &mut probe,
            vocabulary,
            ids,
            false,
            self_pronoun_valid,
            bound_x,
        )?;
        if probe.target != BodyTarget::None {
            flush_clause(&mut active_clause, &mut outer_ops, &mut clause_count)?;
            active_clause = Some(probe);
        } else {
            flush_clause(&mut active_clause, &mut outer_ops, &mut clause_count)?;
            let shift = isize::try_from(outer_ops.len()).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in probe.instructions {
                op.shift_jumps(shift, 0);
                outer_ops.push(op);
            }
        }
        i += step;
    }

    flush_clause(&mut active_clause, &mut outer_ops, &mut clause_count)?;
    if clause_count < 2 || outer_ops.is_empty() {
        return Err("oracle:multiple-targets");
    }
    Ok(ExecutableBody::new(BodyTarget::None, outer_ops))
}

fn emit_condition_branches(
    condition: &ConditionAst,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<Vec<usize>, &'static str> {
    let mut false_branches = Vec::new();
    match condition {
        ConditionAst::And(a, b) => {
            let rc_a = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let rc_b = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_a,
                destination: 0,
            });
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_b,
                destination: 0,
            });
        }
        ConditionAst::Or(a, b) => {
            let mut rc_a = lower_resolution_condition(a, builder, vocabulary, in_trigger, bound_x)?;
            let rc_b = lower_resolution_condition(b, builder, vocabulary, in_trigger, bound_x)?;
            rc_a.comparison = negate_comparison(rc_a.comparison);
            let true_jump_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_a,
                destination: 0,
            });
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: rc_b,
                destination: 0,
            });
            let then_start = builder.instructions.len();
            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                builder.instructions.get_mut(true_jump_idx)
            {
                *destination = then_start;
            }
        }
        other => {
            let res_cond =
                lower_resolution_condition(other, builder, vocabulary, in_trigger, bound_x)?;
            false_branches.push(builder.instructions.len());
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: res_cond,
                destination: 0,
            });
        }
    }
    Ok(false_branches)
}

pub(super) fn lower_instruction_seq(
    ins: &[InstructionAst],
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    let mut standalone_x: Option<&ValueExprAst> = None;
    let mut empty_bind_count = 0usize;
    if bound_x.is_none() {
        for item in ins {
            if let InstructionAst::BindX { x_expr, body } = item
                && body.is_empty()
            {
                standalone_x = Some(x_expr.as_ref());
                empty_bind_count += 1;
            }
        }
    }
    let bound_x = if bound_x.is_none() && empty_bind_count == 1 {
        standalone_x
    } else {
        bound_x
    };

    let mut i = 0usize;
    while i < ins.len() {
        if empty_bind_count == 1
            && matches!(&ins[i], InstructionAst::BindX { body, .. } if body.is_empty())
        {
            i += 1;
            continue;
        }
        // Desugar [base_slice, <replace_carrier>] (+ optional Otherwise)
        // into BranchIfFalse(cond -> base_start) + [OptionalSequence] + alt_ops + Jump(end) + base_ops (+ else_ops).
        // Handles IfPreviousSucceeded/IfPreviousFailed, IfCondition (including Optional([ReplaceEffect(...)])),
        // standalone Optional([ReplaceEffect(...)]), and raw ReplaceEffect(alt) at ins[i + base_len].
        let base_len = base_pair_len(ins, i);
        if i + base_len < ins.len()
            && let Some((prev_cond, is_optional_alt, raw_alt)) = match &ins[i + base_len] {
                InstructionAst::IfPreviousSucceeded(tb) if contains_replace_effect(tb) => {
                    Some((true, false, tb.as_slice()))
                }
                InstructionAst::IfPreviousSucceeded(tb) if is_optional_replace_effect(tb) => {
                    if let [InstructionAst::Optional(inner)] = tb.as_slice() {
                        Some((true, true, inner.as_slice()))
                    } else {
                        None
                    }
                }
                InstructionAst::IfPreviousFailed(tb) if contains_replace_effect(tb) => {
                    Some((false, false, tb.as_slice()))
                }
                InstructionAst::IfPreviousFailed(tb) if is_optional_replace_effect(tb) => {
                    if let [InstructionAst::Optional(inner)] = tb.as_slice() {
                        Some((false, true, inner.as_slice()))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        {
            let base_slice = &ins[i..i + base_len];
            let pair = lower_replace_pair(
                base_slice,
                raw_alt,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if is_optional_alt && pair.alt_ops.is_empty() {
                return Err("oracle:empty-optional");
            }
            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            let branch_idx = builder.instructions.len();
            builder.instructions.push(Instruction::BranchIfFalse {
                condition: ResolutionCondition {
                    left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                    comparison: if prev_cond {
                        Comparison::GreaterOrEqual
                    } else {
                        Comparison::Equal
                    },
                    right: ValueExpr::literal(if prev_cond { 1 } else { 0 }),
                },
                destination: 0,
            });
            if is_optional_alt {
                builder.instructions.push(Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: pair.alt_ops.len() + 1,
                });
            }
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            let jump_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::Jump { destination: 0 });
            let base_start = builder.instructions.len();
            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                builder.instructions.get_mut(branch_idx)
            {
                *destination = base_start;
            }
            let base_shift = isize::try_from(base_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.base_ops {
                op.shift_jumps(base_shift, 0);
                builder.instructions.push(op);
            }
            let end_dest = builder.instructions.len();
            if let Some(Instruction::Jump { destination }) = builder.instructions.get_mut(jump_idx)
            {
                *destination = end_dest;
            }
            i += base_len + 1;
            continue;
        }
        if i + base_len < ins.len()
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + base_len]
            && (contains_replace_effect(then_body) || is_optional_replace_effect(then_body))
        {
            let base_slice = &ins[i..i + base_len];
            let (is_optional_alt, raw_alt) = if let [InstructionAst::Optional(opt_inner)] =
                then_body.as_slice()
                && contains_replace_effect(opt_inner)
            {
                (true, opt_inner.as_slice())
            } else {
                (false, then_body.as_slice())
            };
            let pair = lower_replace_pair(
                base_slice,
                raw_alt,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if is_optional_alt && pair.alt_ops.is_empty() {
                return Err("oracle:empty-optional");
            }

            let mut cond_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
            cond_builder.target = pair.merged_target;
            let false_branches = emit_condition_branches(
                condition,
                &mut cond_builder,
                vocabulary,
                in_trigger,
                bound_x,
            )?;
            if cond_builder.target != pair.merged_target {
                return Err("oracle:replace-effect-cond-target-mismatch");
            }

            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }

            let cond_start = builder.instructions.len();
            let cond_shift = isize::try_from(cond_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in cond_builder.instructions {
                op.shift_jumps(cond_shift, 0);
                builder.instructions.push(op);
            }
            if is_optional_alt {
                builder.instructions.push(Instruction::OptionalSequence {
                    chooser: ResolvingPlayer::Controller,
                    instructions: pair.alt_ops.len() + 1,
                });
            }
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            let jump_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::Jump { destination: 0 });
            let base_start = builder.instructions.len();
            for rel_idx in false_branches {
                if let Some(Instruction::BranchIfFalse { destination, .. }) =
                    builder.instructions.get_mut(cond_start + rel_idx)
                {
                    *destination = base_start;
                }
            }
            let base_shift = isize::try_from(base_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.base_ops {
                op.shift_jumps(base_shift, 0);
                builder.instructions.push(op);
            }
            let mut step = base_len + 1;
            if i + step < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + step]
            {
                let unwrapped_else: Vec<InstructionAst>;
                let else_slice: &[InstructionAst] = if contains_replace_effect(else_body) {
                    unwrapped_else =
                        substitute_those_tokens(&strip_replace_effect(else_body), base_slice);
                    &unwrapped_else
                } else {
                    else_body
                };
                lower_instruction_seq(
                    else_slice,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                step += 1;
            }
            let end_dest = builder.instructions.len();
            if let Some(Instruction::Jump { destination }) = builder.instructions.get_mut(jump_idx)
            {
                *destination = end_dest;
            }
            i += step;
            continue;
        }
        if i + base_len < ins.len()
            && let InstructionAst::Optional(opt_inner) = &ins[i + base_len]
            && contains_replace_effect(opt_inner)
        {
            let base_slice = &ins[i..i + base_len];
            let pair = lower_replace_pair(
                base_slice,
                opt_inner,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if pair.alt_ops.is_empty() {
                return Err("oracle:empty-optional");
            }
            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            builder.instructions.push(Instruction::OptionalSequence {
                chooser: ResolvingPlayer::Controller,
                instructions: pair.alt_ops.len() + 1,
            });
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            let jump_idx = builder.instructions.len();
            builder
                .instructions
                .push(Instruction::Jump { destination: 0 });
            let base_start = builder.instructions.len();
            let base_shift = isize::try_from(base_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.base_ops {
                op.shift_jumps(base_shift, 0);
                builder.instructions.push(op);
            }
            let end_dest = builder.instructions.len();
            if let Some(Instruction::Jump { destination }) = builder.instructions.get_mut(jump_idx)
            {
                *destination = end_dest;
            }
            i += base_len + 1;
            continue;
        }
        if i + base_len < ins.len()
            && let InstructionAst::ReplaceEffect(alt_inner) = &ins[i + base_len]
        {
            let base_slice = &ins[i..i + base_len];
            let pair = lower_replace_pair(
                base_slice,
                alt_inner,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if builder.target == BodyTarget::None && pair.merged_target != BodyTarget::None {
                builder.bind_target(pair.merged_target)?;
            }
            if builder.graveyard_target_bounds.is_none() {
                builder.graveyard_target_bounds = pair.merged_gy;
            }
            builder.x_target_count |= pair.x_target_count;
            if builder.last_group_filter.is_none() {
                builder.last_group_filter = pair.last_group_filter;
            }
            let alt_start = builder.instructions.len();
            let alt_shift = isize::try_from(alt_start).map_err(|_| "oracle:jump-shift-ovf")?;
            for mut op in pair.alt_ops {
                op.shift_jumps(alt_shift, 0);
                builder.instructions.push(op);
            }
            i += base_len + 1;
            continue;
        }
        // Fuse OneShotReplacement peepholes:
        // 1. [Counter, OneShotReplacement(countered -> exile)] -> CounterSpellToExile (Deny the Divine)
        // 2. [DealDamage(self -> single/any target), OneShotReplacement(die this turn -> exile)] -> DamageAmountThenExileIfDies (Yamabushi's Flame)
        if i + 1 < ins.len()
            && let InstructionAst::OneShotReplacement {
                event,
                replace_with,
                instead_body: None,
                ..
            } = &ins[i + 1]
        {
            let ev_low = event.trim().to_ascii_lowercase();
            let rep_low = replace_with.trim().to_ascii_lowercase();
            if ev_low.contains("counter")
                && rep_low.contains("exile")
                && matches!(&ins[i], InstructionAst::Counter { .. })
            {
                lower_instruction(
                    &ins[i],
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                if let Some(last @ Instruction::CounterSpell) = builder.instructions.last_mut() {
                    *last = Instruction::CounterSpellToExile;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if (ev_low.contains("die") || ev_low.contains("graveyard"))
                && rep_low.contains("exile")
                && let InstructionAst::DealDamage {
                    source,
                    target,
                    amount,
                } = &ins[i]
                && is_self_pronoun(source, builder, self_pronoun_valid)
            {
                let prev_len = builder.instructions.len();
                let mut probe = builder.clone();
                if lower_instruction(
                    &InstructionAst::DealDamage {
                        source: source.clone(),
                        target: target.clone(),
                        amount: amount.clone(),
                    },
                    &mut probe,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && probe.instructions.len() == prev_len + 1
                {
                    let upgraded = match probe.instructions.last() {
                        Some(Instruction::DamageAmount(amt)) => {
                            Some(Instruction::DamageAmountThenExileIfDies(amt.clone()))
                        }
                        Some(Instruction::Damage { amount }) => {
                            Some(Instruction::DamageAmountThenExileIfDies(
                                ValueExpr::Literal(amount.clone()),
                            ))
                        }
                        _ => None,
                    };
                    if let Some(op) = upgraded {
                        *builder = probe;
                        if let Some(last) = builder.instructions.last_mut() {
                            *last = op;
                        }
                        i += 2;
                        continue;
                    }
                }
            }
        }
        // Fuse paired ModifyPT + GrantKeywords on the exact same subject & duration
        // (emitted by clauses.rs for "Target creature gets +1/+1 and gains flying until end of turn").
        if i + 1 < ins.len()
            && let InstructionAst::ModifyPT {
                subject: s1,
                duration: d1,
                ..
            } = &ins[i]
            && let InstructionAst::GrantKeywords {
                subject: s2,
                keywords,
                duration: d2,
            } = &ins[i + 1]
            && s1 == s2
            && d1 == d2
        {
            lower_instruction(
                &ins[i],
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            let ab_change = lower_ability_change(keywords, vocabulary)?;
            let pure_kws = lower_keyword_list(keywords).ok();
            match builder.instructions.last_mut() {
                Some(Instruction::BoostSource {
                    keywords: prev_kws, ..
                }) if pure_kws.is_some() => {
                    if let Some(kws) = pure_kws {
                        prev_kws.extend(kws);
                    }
                    i += 2;
                    continue;
                }
                Some(Instruction::BoostTargetPower {
                    keywords: prev_kws, ..
                }) if pure_kws.is_some() => {
                    if let Some(kws) = pure_kws {
                        prev_kws.extend(kws);
                    }
                    i += 2;
                    continue;
                }
                Some(Instruction::ModifyGroupUntilEndOfTurn {
                    keywords: prev_kws, ..
                }) if pure_kws.is_some() => {
                    if let Some(kws) = pure_kws {
                        prev_kws.extend(kws);
                    }
                    i += 2;
                    continue;
                }
                Some(Instruction::CreateContinuousEffect { parts, .. })
                | Some(Instruction::ChangeAbilitySource { parts, .. })
                | Some(Instruction::ChangeGroup { parts, .. })
                | Some(Instruction::ModifyAnyNumberOfPermanents { parts, .. })
                | Some(Instruction::AffectPermanentTargets {
                    action: PermanentTargetAction::Modify { parts, .. },
                    ..
                }) if !parts
                    .0
                    .iter()
                    .any(|p| matches!(p, ResolvedPartSpec::Abilities(_))) =>
                {
                    parts.0.push(ResolvedPartSpec::Abilities(ab_change));
                    i += 2;
                    continue;
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        // Fuse [Optional([SearchLibrary(shuffle: false, to_zone: "found_set")])] + ShuffleLibrary + MoveZone(Anaphora, "top...")
        if i + 2 < ins.len()
            && let InstructionAst::Optional(inner) = &ins[i]
            && let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: false,
                    up_to,
                    any_number,
                },
            ] = inner.as_slice()
            && (to_zone == "found_set" || to_zone.is_empty())
            && matches!(
                &ins[i + 1],
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You
                }
            )
            && let InstructionAst::MoveZone { subject, to } = &ins[i + 2]
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            && to.to_ascii_lowercase().contains("top")
        {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                "top of your library",
                vocabulary,
                true,
                *up_to,
                *any_number,
            )?;
            builder.instructions.push(Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            });
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 3;
            continue;
        }
        // Fuse SearchLibrary(shuffle: false, to_zone: "found_set") + ShuffleLibrary + MoveZone(Anaphora, "top...")
        if i + 2 < ins.len()
            && let InstructionAst::SearchLibrary {
                library,
                count,
                filter,
                reveal,
                to_zone,
                shuffle: false,
                up_to,
                any_number,
            } = &ins[i]
            && (to_zone == "found_set" || to_zone.is_empty())
            && matches!(
                &ins[i + 1],
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You
                        | PlayerRefAst::ThatPlayer
                        | PlayerRefAst::TargetPlayer
                        | PlayerRefAst::TargetOpponent
                        | PlayerRefAst::EachPlayer
                        | PlayerRefAst::EachOpponent
                }
            )
            && let InstructionAst::MoveZone { subject, to } = &ins[i + 2]
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            && to.to_ascii_lowercase().contains("top")
        {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                "top of your library",
                vocabulary,
                false,
                *up_to,
                *any_number,
            )?;
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 3;
            continue;
        }
        // Fuse Optional([SearchLibrary(shuffle: _, to_zone: "found_set")]) + MoveZone(Anaphora, "top...")
        if i + 1 < ins.len()
            && let InstructionAst::Optional(inner) = &ins[i]
            && let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle,
                    up_to,
                    any_number,
                },
            ] = inner.as_slice()
            && (to_zone == "found_set" || to_zone.is_empty())
            && let InstructionAst::MoveZone { subject, to } = &ins[i + 1]
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            && {
                let to_low = to.to_ascii_lowercase();
                to_low.contains("top") && (*shuffle || to_low.contains("after_shuffle"))
            }
        {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                "top of your library",
                vocabulary,
                true,
                *up_to,
                *any_number,
            )?;
            builder.instructions.push(Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            });
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 2;
            continue;
        }
        // Fuse SearchLibrary(shuffle: _, to_zone: "found_set") + MoveZone(Anaphora, "top...")
        if i + 1 < ins.len()
            && let InstructionAst::SearchLibrary {
                library,
                count,
                filter,
                reveal,
                to_zone,
                shuffle,
                up_to,
                any_number,
            } = &ins[i]
            && (to_zone == "found_set" || to_zone.is_empty())
            && let InstructionAst::MoveZone { subject, to } = &ins[i + 1]
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            && {
                let to_low = to.to_ascii_lowercase();
                to_low.contains("top") && (*shuffle || to_low.contains("after_shuffle"))
            }
        {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                "top of your library",
                vocabulary,
                false,
                *up_to,
                *any_number,
            )?;
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 2;
            continue;
        }
        // Fuse Optional([SearchLibrary(shuffle: false)]) + ShuffleLibrary(You)
        if i + 1 < ins.len()
            && let InstructionAst::Optional(inner) = &ins[i]
            && let [
                InstructionAst::SearchLibrary {
                    library,
                    count,
                    filter,
                    reveal,
                    to_zone,
                    shuffle: false,
                    up_to,
                    any_number,
                },
            ] = inner.as_slice()
            && matches!(
                &ins[i + 1],
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You
                }
            )
        {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                to_zone,
                vocabulary,
                true,
                *up_to,
                *any_number,
            )?;
            builder.instructions.push(Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            });
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 2;
            continue;
        }
        // Fuse SearchLibrary(shuffle: false) + ShuffleLibrary into native SearchLibrary
        if i + 1 < ins.len()
            && let InstructionAst::SearchLibrary {
                library,
                count,
                filter,
                reveal,
                to_zone,
                shuffle: false,
                up_to,
                any_number,
            } = &ins[i]
            && matches!(
                &ins[i + 1],
                InstructionAst::ShuffleLibrary {
                    player: PlayerRefAst::You
                        | PlayerRefAst::ThatPlayer
                        | PlayerRefAst::TargetPlayer
                        | PlayerRefAst::TargetOpponent
                        | PlayerRefAst::EachPlayer
                        | PlayerRefAst::EachOpponent
                }
            )
        {
            let search = lower_library_search(
                library,
                count,
                filter,
                *reveal,
                to_zone,
                vocabulary,
                false,
                *up_to,
                *any_number,
            )?;
            builder.instructions.push(Instruction::SearchLibrary {
                filter: Box::new(search),
            });
            i += 2;
            continue;
        }
        // Fuse Exile(target) + DelayedTrigger(EndStep, ReturnZone(Anaphora, battlefield)) -> ExileUntilNextEndStep
        if i + 1 < ins.len()
            && let InstructionAst::Exile {
                actor: None | Some(PlayerRefAst::You),
                subject,
            } = &ins[i]
            && let SubjectRefAst::TargetObject {
                filter,
                count: TargetCountAst::Exact(1),
            } = subject.as_ref()
            && filter.zone.is_none()
            && let InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                scope: DelayedStepScope::Next,
                body: delayed_body,
            } = &ins[i + 1]
            && let [
                InstructionAst::ReturnZone {
                    subject: ret_subj,
                    to,
                    tapped: false,
                    transformed: false,
                    with_counter: None,
                    attached_to: None,
                    ..
                },
            ] = delayed_body.as_slice()
            && matches!(ret_subj.as_ref(), SubjectRefAst::Anaphora(_))
            && matches!(
                to.trim().to_ascii_lowercase().as_str(),
                "battlefield" | "the battlefield" | "the battlefield under its owner's control"
            )
        {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::ExileUntilNextEndStep);
            i += 2;
            continue;
        }
        // Fuse FlipCoin + IfCondition(WonContest) (+ optional Otherwise) -> FlipCoin { lose_destination }
        if i + 1 < ins.len()
            && matches!(&ins[i], InstructionAst::FlipCoin)
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + 1]
            && matches!(condition.as_ref(), ConditionAst::WonContest)
        {
            let flip_idx = builder.instructions.len();
            builder.instructions.push(Instruction::FlipCoin {
                lose_destination: 0,
            });
            lower_instruction_seq(
                then_body,
                builder,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            if i + 2 < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
            {
                let jump_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::Jump { destination: 0 });
                let lose_dest = builder.instructions.len();
                if let Some(Instruction::FlipCoin { lose_destination }) =
                    builder.instructions.get_mut(flip_idx)
                {
                    *lose_destination = lose_dest;
                }
                lower_instruction_seq(
                    else_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::Jump { destination }) =
                    builder.instructions.get_mut(jump_idx)
                {
                    *destination = end_dest;
                }
                i += 3;
            } else {
                let lose_dest = builder.instructions.len();
                if let Some(Instruction::FlipCoin { lose_destination }) =
                    builder.instructions.get_mut(flip_idx)
                {
                    *lose_destination = lose_dest;
                }
                i += 2;
            }
            continue;
        }
        // Lower IfCondition (+ optional Otherwise) via BranchIfFalse + Jump (including And/Or).
        // When the condition references target anaphora (e.g. Fatal Push / Overload:
        // "Destroy target creature if it has mana value 2 or less") and no target is bound yet,
        // probe then_body first to bind the target before lowering the condition.
        if let InstructionAst::IfCondition {
            condition,
            then_body,
        } = &ins[i]
        {
            let prev_len = builder.instructions.len();
            let prev_target = builder.target;
            let prev_gy = builder.graveyard_target_bounds;
            let prev_x = builder.x_target_count;
            let prev_group = builder.last_group_filter;
            let (false_branches, lowered_then_already) = match emit_condition_branches(
                condition, builder, vocabulary, in_trigger, bound_x,
            ) {
                Ok(fb) => (fb, false),
                Err(cond_err) if prev_target == BodyTarget::None => {
                    builder.instructions.truncate(prev_len);
                    builder.target = prev_target;
                    builder.graveyard_target_bounds = prev_gy;
                    builder.x_target_count = prev_x;
                    builder.last_group_filter = prev_group;
                    let mut then_builder = BodyBuilder::with_trigger_hint(builder.trigger_hint);
                    then_builder.last_group_filter = prev_group;
                    lower_instruction_seq(
                        then_body,
                        &mut then_builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                    if then_builder.target == BodyTarget::None {
                        return Err(cond_err);
                    }
                    builder.bind_target(then_builder.target)?;
                    let fb = emit_condition_branches(
                        condition, builder, vocabulary, in_trigger, bound_x,
                    )?;
                    if builder.target != then_builder.target {
                        return Err("oracle:conflicting-targets");
                    }
                    if builder.graveyard_target_bounds.is_none() {
                        builder.graveyard_target_bounds = then_builder.graveyard_target_bounds;
                    }
                    builder.x_target_count |= then_builder.x_target_count;
                    if then_builder.last_group_filter.is_some() {
                        builder.last_group_filter = then_builder.last_group_filter;
                    }
                    let then_start = builder.instructions.len();
                    let then_shift =
                        isize::try_from(then_start).map_err(|_| "oracle:jump-shift-ovf")?;
                    for mut op in then_builder.instructions {
                        op.shift_jumps(then_shift, 0);
                        builder.instructions.push(op);
                    }
                    (fb, true)
                }
                Err(cond_err) => return Err(cond_err),
            };
            if !lowered_then_already {
                lower_instruction_seq(
                    then_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
            }
            if i + 1 < ins.len()
                && let InstructionAst::Otherwise(else_body) = &ins[i + 1]
            {
                let jump_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(Instruction::Jump { destination: 0 });
                let else_dest = builder.instructions.len();
                for b_idx in false_branches {
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(b_idx)
                    {
                        *destination = else_dest;
                    }
                }
                let unwrapped_else: Vec<InstructionAst>;
                let else_slice: &[InstructionAst] = if contains_replace_effect(else_body) {
                    unwrapped_else =
                        substitute_those_tokens(&strip_replace_effect(else_body), then_body);
                    &unwrapped_else
                } else {
                    else_body
                };
                lower_instruction_seq(
                    else_slice,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::Jump { destination }) =
                    builder.instructions.get_mut(jump_idx)
                {
                    *destination = end_dest;
                }
                i += 2;
            } else {
                let end_dest = builder.instructions.len();
                for b_idx in false_branches {
                    if let Some(Instruction::BranchIfFalse { destination, .. }) =
                        builder.instructions.get_mut(b_idx)
                    {
                        *destination = end_dest;
                    }
                }
                i += 1;
            }
            continue;
        }
        // Unified Optional / OptionalPlayer starting with PayCost(PayMana) or PlayerPayCost(PayMana):
        // Never wrap OfferManaPayment / OfferDynamicManaPayment in OptionalEffect (program.rs rejects it).
        let opt_mana_head: Option<(PlayerRefAst, &str, &[InstructionAst])> = match &ins[i] {
            InstructionAst::Optional(opt_inner) => match opt_inner.first() {
                Some(InstructionAst::PayCost(CostAtomAst::PayMana(raw))) => {
                    Some((PlayerRefAst::You, raw.as_str(), &opt_inner[1..]))
                }
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::PayMana(raw),
                }) => Some((*player, raw.as_str(), &opt_inner[1..])),
                _ => None,
            },
            InstructionAst::OptionalPlayer {
                player: opt_player,
                body: opt_inner,
            } => match opt_inner.first() {
                Some(InstructionAst::PayCost(CostAtomAst::PayMana(raw))) => {
                    Some((*opt_player, raw.as_str(), &opt_inner[1..]))
                }
                Some(InstructionAst::PlayerPayCost {
                    player,
                    cost: CostAtomAst::PayMana(raw),
                }) => Some((*player, raw.as_str(), &opt_inner[1..])),
                _ => None,
            },
            _ => None,
        };
        if let Some((payer_ast, raw_mana, opt_rest)) = opt_mana_head {
            let payer = lower_resolving_player(payer_ast, builder)?;
            let offer_template = try_build_offer_mana(payer, raw_mana, 0, 0, bound_x, vocabulary)
                .ok_or("oracle:unsupported-pay-cost")?;
            if !opt_rest.is_empty() {
                let offer_idx = builder.instructions.len();
                builder
                    .instructions
                    .push(with_offer_dests(offer_template, offer_idx + 1, 0));
                lower_instruction_seq(
                    opt_rest,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                set_offer_declined(builder.instructions.get_mut(offer_idx), end_dest);
                i += 1;
                continue;
            }
            if i + 1 < ins.len() {
                match &ins[i + 1] {
                    InstructionAst::IfPreviousSucceeded(then_body) => {
                        let offer_idx = builder.instructions.len();
                        builder.instructions.push(with_offer_dests(
                            offer_template,
                            offer_idx + 1,
                            0,
                        ));
                        lower_instruction_seq(
                            then_body,
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                        if i + 2 < ins.len()
                            && let InstructionAst::Otherwise(else_body) = &ins[i + 2]
                        {
                            let jump_idx = builder.instructions.len();
                            builder
                                .instructions
                                .push(Instruction::Jump { destination: 0 });
                            let else_start = builder.instructions.len();
                            set_offer_declined(builder.instructions.get_mut(offer_idx), else_start);
                            lower_instruction_seq(
                                else_body,
                                builder,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )?;
                            let end_dest = builder.instructions.len();
                            if let Some(Instruction::Jump { destination }) =
                                builder.instructions.get_mut(jump_idx)
                            {
                                *destination = end_dest;
                            }
                            i += 3;
                        } else {
                            let end_dest = builder.instructions.len();
                            set_offer_declined(builder.instructions.get_mut(offer_idx), end_dest);
                            i += 2;
                        }
                        continue;
                    }
                    InstructionAst::IfPreviousFailed(else_body) => {
                        let offer_idx = builder.instructions.len();
                        builder.instructions.push(with_offer_dests(
                            offer_template,
                            0,
                            offer_idx + 1,
                        ));
                        lower_instruction_seq(
                            else_body,
                            builder,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )?;
                        let end_dest = builder.instructions.len();
                        set_offer_paid(builder.instructions.get_mut(offer_idx), end_dest);
                        i += 2;
                        continue;
                    }
                    InstructionAst::ReflexiveTrigger(ref_body) => {
                        let reflexive_body =
                            lower_effect_body(ref_body, vocabulary, ids, true, self_pronoun_valid)?;
                        let offer_idx = builder.instructions.len();
                        builder.instructions.push(with_offer_dests(
                            offer_template,
                            offer_idx + 1,
                            offer_idx + 2,
                        ));
                        builder
                            .instructions
                            .push(Instruction::EmitReflexiveTrigger {
                                body: Box::new(reflexive_body),
                            });
                        i += 2;
                        continue;
                    }
                    _ => {}
                }
            }
            // Standalone optional mana payment: emit OfferManaPayment directly without OptionalEffect.
            let offer_idx = builder.instructions.len();
            builder.instructions.push(with_offer_dests(
                offer_template,
                offer_idx + 1,
                offer_idx + 1,
            ));
            i += 1;
            continue;
        }
        // Lower ReflexiveTrigger ("When you do, ...") after an action/optional cost with CR 603.12 conditional guard
        if let InstructionAst::ReflexiveTrigger(ref_body) = &ins[i] {
            let prev_tracks_action = i > 0
                && matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_)
                        | InstructionAst::OptionalPlayer { .. }
                        | InstructionAst::Sacrifice { .. }
                        | InstructionAst::Discard { .. }
                        | InstructionAst::DiscardHand { .. }
                        | InstructionAst::GainPlayerCounters { .. }
                        | InstructionAst::PayCost(_)
                        | InstructionAst::RemoveCounters { .. }
                        | InstructionAst::SearchLibrary { .. }
                        | InstructionAst::Clash { .. }
                );
            let reflexive_body =
                lower_effect_body(ref_body, vocabulary, ids, true, self_pronoun_valid)?;
            if prev_tracks_action {
                let branch_idx = builder.instructions.len();
                builder.instructions.push(Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::event(EventValueKind::LastActionSucceeded),
                        comparison: Comparison::GreaterOrEqual,
                        right: ValueExpr::literal(1),
                    },
                    destination: branch_idx + 2,
                });
            }
            builder
                .instructions
                .push(Instruction::EmitReflexiveTrigger {
                    body: Box::new(reflexive_body),
                });
            i += 1;
            continue;
        }
        // Lower IfPreviousSucceeded / IfPreviousFailed right after an action that tracks last_action_succeeded
        if let InstructionAst::IfPreviousSucceeded(then_body)
        | InstructionAst::IfPreviousFailed(then_body) = &ins[i]
        {
            let succeeded = matches!(&ins[i], InstructionAst::IfPreviousSucceeded(_));
            let prev_was_optional = i > 0
                && matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_) | InstructionAst::OptionalPlayer { .. }
                );
            let prev_tracks_action = i > 0
                && matches!(
                    &ins[i - 1],
                    InstructionAst::Optional(_)
                        | InstructionAst::OptionalPlayer { .. }
                        | InstructionAst::Sacrifice { .. }
                        | InstructionAst::Discard { .. }
                        | InstructionAst::DiscardHand { .. }
                        | InstructionAst::GainPlayerCounters { .. }
                        | InstructionAst::PayCost(_)
                        | InstructionAst::RemoveCounters { .. }
                        | InstructionAst::SearchLibrary { .. }
                        | InstructionAst::Clash { .. }
                );
            if prev_tracks_action {
                let is_single_sacrifice = !prev_was_optional
                    && matches!(
                        builder.instructions.last(),
                        Some(Instruction::Sacrifice {
                            players: AffectedPlayers::One(_),
                            amount: ValueExpr::Literal(v),
                            ..
                        }) if *v == 1.into()
                    );
                let kind = if succeeded && is_single_sacrifice {
                    EventValueKind::LastSacrificeSucceeded
                } else {
                    EventValueKind::LastActionSucceeded
                };
                let branch_idx = builder.instructions.len();
                builder.instructions.push(Instruction::BranchIfFalse {
                    condition: ResolutionCondition {
                        left: ValueExpr::event(kind),
                        comparison: if succeeded {
                            Comparison::GreaterOrEqual
                        } else {
                            Comparison::Equal
                        },
                        right: ValueExpr::literal(if succeeded { 1 } else { 0 }),
                    },
                    destination: 0,
                });
                lower_instruction_seq(
                    then_body,
                    builder,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )?;
                let end_dest = builder.instructions.len();
                if let Some(Instruction::BranchIfFalse { destination, .. }) =
                    builder.instructions.get_mut(branch_idx)
                {
                    *destination = end_dest;
                }
                i += 1;
                continue;
            }
        }
        // Lower DelayedTrigger right after CreateTokens / CreateAttackingTokens for sacrifice/exile tokens
        if let InstructionAst::DelayedTrigger {
            step,
            scope: DelayedStepScope::Next,
            body,
        } = &ins[i]
            && matches!(
                builder.instructions.last(),
                Some(Instruction::CreateTokens { .. } | Instruction::CreateAttackingTokens { .. })
            )
        {
            match (step, body.as_slice()) {
                (
                    TurnStepAst::EndStep,
                    [
                        InstructionAst::Sacrifice {
                            player: PlayerRefAst::You,
                            count: None,
                            subject,
                        },
                    ],
                ) if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::Group(_)
                ) =>
                {
                    builder
                        .instructions
                        .push(Instruction::ScheduleCreatedTokenSacrifice);
                    i += 1;
                    continue;
                }
                (
                    TurnStepAst::EndStep | TurnStepAst::EndOfCombat,
                    [
                        InstructionAst::Exile {
                            actor: None | Some(PlayerRefAst::You),
                            subject,
                        },
                    ],
                ) if matches!(
                    subject.as_ref(),
                    SubjectRefAst::Anaphora(_) | SubjectRefAst::Group(_)
                ) =>
                {
                    let eng_step = if *step == TurnStepAst::EndOfCombat {
                        Step::EndCombat
                    } else {
                        Step::End
                    };
                    builder
                        .instructions
                        .push(Instruction::ScheduleCreatedTokenExile(eng_step));
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }
        // Fuse paired impulse draw: ExileTopLibrary + GrantCastPermission (+ optional ManaSpendRestriction) -> ExileTopForPlay / ExileTopPlay
        if i + 1 < ins.len()
            && let InstructionAst::ExileTopLibrary {
                actor: None | Some(PlayerRefAst::You),
                whose,
                count,
                face_down,
            } = &ins[i]
            && let Some(InstructionAst::GrantCastPermission {
                player: PlayerRefAst::You,
                cast_only,
                subject,
                free,
                duration,
            }) = (match &ins[i + 1] {
                g @ InstructionAst::GrantCastPermission { .. } => Some(g),
                InstructionAst::Optional(inner) if inner.len() == 1 => {
                    matches!(&inner[0], InstructionAst::GrantCastPermission { .. })
                        .then_some(&inner[0])
                }
                _ => None,
            })
            && matches!(
                subject.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.trim().to_ascii_lowercase().as_str(),
                    "it" | "them" | "that card" | "those cards" | "the exiled card" | "the exiled cards" | "cards exiled this way" | "a card exiled this way"
                )
            )
        {
            let dur_low = duration.trim().to_ascii_lowercase();
            let parsed_dur = if dur_low == "until_end_of_next_turn"
                || dur_low.contains("end of your next turn")
                || dur_low.contains("untilendofnextturn")
                || dur_low.contains("untilyournextturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::UntilEndOfNextTurn)
            } else if dur_low == "this_turn"
                || dur_low == "until_end_of_turn"
                || dur_low.contains("this turn")
                || dur_low.contains("end of turn")
                || dur_low.contains("untilendofturn")
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::ThisTurn)
            } else if dur_low == "while_exiled"
                || dur_low.contains("remains exiled")
                || dur_low.contains("remain exiled")
                || dur_low == "permanent"
            {
                Some(spellwake_engine::program::ImpulsePlayDuration::WhileExiled)
            } else if dur_low == "until_next_exile" || dur_low.contains("until you exile another") {
                Some(spellwake_engine::program::ImpulsePlayDuration::UntilNextFromSource)
            } else {
                None
            };
            let w_low = whose.trim().to_ascii_lowercase();
            let owner_spec: Option<(Option<BodyTarget>, AffectedPlayers)> = match w_low.as_str() {
                "your" | "own" | "" => {
                    Some((None, AffectedPlayers::One(ResolvingPlayer::Controller)))
                }
                "target player" | "target player's" => Some((
                    Some(BodyTarget::SinglePlayer),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                )),
                "target opponent" | "target opponent's" => Some((
                    Some(BodyTarget::SingleOpponent),
                    AffectedPlayers::One(ResolvingPlayer::TargetPlayer),
                )),
                "each opponent" | "each opponent's" => Some((None, AffectedPlayers::EachOpponent)),
                "each player" | "each player's" => Some((None, AffectedPlayers::EachPlayer)),
                "each other player" | "each other player's" => {
                    Some((None, AffectedPlayers::EachOtherPlayer))
                }
                "that player" | "that player's" | "defending player" | "defending player's"
                | "enchanted player" | "enchanted player's" | "its controller"
                | "its controller's" => Some((
                    None,
                    AffectedPlayers::One(ResolvingPlayer::UniqueRelatedPlayer),
                )),
                _ => None,
            };
            if let (Some(impulse_dur), Some((req_target, library_owner))) = (parsed_dur, owner_spec)
            {
                if let Some(bt) = req_target {
                    builder.bind_target(bt)?;
                }
                let mut any_color = false;
                let mut step = 2usize;
                if i + 2 < ins.len()
                    && let InstructionAst::ManaSpendRestriction(r) = &ins[i + 2]
                {
                    let rl = r.to_ascii_lowercase();
                    if rl.contains("any color") || rl.contains("any type") {
                        any_color = true;
                        step = 3;
                    }
                }
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                if cnt == ValueExpr::literal(1)
                    && library_owner == AffectedPlayers::One(ResolvingPlayer::Controller)
                    && !*cast_only
                    && !*free
                    && !any_color
                    && !*face_down
                    && impulse_dur == spellwake_engine::program::ImpulsePlayDuration::ThisTurn
                {
                    builder.instructions.push(Instruction::ExileTopForPlay {
                        until_next_from_source: false,
                    });
                } else {
                    builder.instructions.push(Instruction::ExileTopPlay {
                        count: cnt,
                        library_owner,
                        duration: impulse_dur,
                        cast_only: *cast_only,
                        free: *free,
                        any_color,
                        face_down: *face_down,
                        grant_play: true,
                    });
                }
                i += step;
                continue;
            }
        }
        // Fuse paired drain life:
        // "Each opponent loses N life and you gain N life" / "Target player loses N life and you gain N life"
        if i + 1 < ins.len()
            && let InstructionAst::ChangeLife {
                player: loser,
                delta: lose_amt,
                gain: false,
            } = &ins[i]
            && let InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                delta: gain_amt,
                gain: true,
            } = &ins[i + 1]
            && (lose_amt == gain_amt
                || (matches!(gain_amt.as_ref(), ValueExprAst::PriorResult)
                    && !matches!(loser, PlayerRefAst::EachOpponent)))
        {
            let amt = lower_value_expr(resolve_x(lose_amt, bound_x), vocabulary)?;
            match loser {
                PlayerRefAst::EachOpponent => {
                    builder
                        .instructions
                        .push(Instruction::OpponentsLoseAndControllerGains(amt));
                    i += 2;
                    continue;
                }
                PlayerRefAst::TargetPlayer => {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                    builder
                        .instructions
                        .push(Instruction::DrainTargetAndGain(amt));
                    i += 2;
                    continue;
                }
                PlayerRefAst::TargetOpponent => {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                    builder
                        .instructions
                        .push(Instruction::DrainTargetAndGain(amt));
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        // Fuse RevealUntil / ExileOrRevealTopUntil(You, filter, n) + ChooseAndKeep / MoveZone (+ rest MoveZone or Shuffle) -> RevealUntilMatch
        let until_header: Option<(&ObjectFilterAst, u8, bool)> = match &ins[i] {
            InstructionAst::RevealUntil {
                player: PlayerRefAst::You,
                filter,
                count,
            } => match count.as_ref() {
                ValueExprAst::Const(n_cnt) if *n_cnt >= 1 => u8::try_from(*n_cnt)
                    .ok()
                    .map(|c| (filter.as_ref(), c, false)),
                _ => None,
            },
            InstructionAst::ExileOrRevealTopUntil {
                exile,
                library,
                filter,
            } if matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            ) =>
            {
                Some((filter.as_ref(), 1, *exile))
            }
            _ => None,
        };
        if let Some((until_filter, count_u8, exile_instead)) = until_header
            && let Ok(obj_filter) = lower_object_filter(until_filter, vocabulary)
        {
            // Pattern A: 2-step RevealUntil + ChooseAndKeep (e.g. Hermit Druid)
            if i + 1 < ins.len()
                && let InstructionAst::ChooseAndKeep {
                    pick,
                    keep_to,
                    rest_to,
                } = &ins[i + 1]
            {
                let pick_low = pick.trim().to_ascii_lowercase();
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let keep_all = pick_low.contains("all cards revealed")
                    || pick_low.contains("all cards exiled")
                    || pick_low == "all of them"
                    || pick_low == "them";
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let random_rest = rest_low.contains("random order");
                if (to_hand || to_battlefield) && (rest_bottom || rest_gy || rest_exile || keep_all)
                {
                    builder.instructions.push(Instruction::RevealUntilMatch {
                        filter: Box::new(obj_filter),
                        count: count_u8,
                        to_battlefield,
                        enters_tapped,
                        keep_all_revealed: keep_all,
                        optional_keep: false,
                        exile_instead_of_reveal: exile_instead,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                        rest_to_bottom: rest_bottom,
                        random_rest_order: random_rest,
                    });
                    i += 2;
                    continue;
                }
            }
            // Pattern B: 2- or 3-step RevealUntil + [Optional?]MoveZone/ReturnZone(that card / all revealed -> hand/bf) + [MoveZone(rest -> bottom/gy/exile) | ShuffleLibrary]
            if i + 1 < ins.len() {
                let (move_subj, move_to, optional_put, tapped_flag) = match &ins[i + 1] {
                    InstructionAst::MoveZone { subject, to } => {
                        (Some(subject.as_ref()), Some(to.as_str()), false, false)
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    } => (Some(subject.as_ref()), Some(to.as_str()), false, *tapped),
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::MoveZone { subject, to }] => {
                            (Some(subject.as_ref()), Some(to.as_str()), true, false)
                        }
                        [
                            InstructionAst::ReturnZone {
                                subject,
                                to,
                                tapped,
                                transformed: false,
                                with_counter: None,
                                attached_to: None,
                                ..
                            },
                        ] => (Some(subject.as_ref()), Some(to.as_str()), true, *tapped),
                        _ => (None, None, false, false),
                    },
                    _ => (None, None, false, false),
                };
                if let (Some(SubjectRefAst::Anaphora(a)), Some(keep_to)) = (move_subj, move_to) {
                    let a_low = a.trim().to_ascii_lowercase();
                    let keep_all = a_low.contains("all cards revealed")
                        || a_low.contains("all revealed")
                        || a_low.contains("all cards exiled")
                        || a_low.contains("all exiled")
                        || a_low == "them"
                        || a_low == "those cards";
                    let single_card = a_low.contains("that card")
                        || a_low == "it"
                        || a_low.contains("revealed card")
                        || a_low.contains("exiled card");
                    let keep_low = keep_to.to_ascii_lowercase();
                    let to_hand = keep_low.contains("hand");
                    let to_battlefield = keep_low.contains("battlefield");
                    let enters_tapped =
                        to_battlefield && (tapped_flag || keep_low.contains("tapped"));
                    if (to_hand || to_battlefield) && keep_all && !optional_put {
                        builder.instructions.push(Instruction::RevealUntilMatch {
                            filter: Box::new(obj_filter),
                            count: count_u8,
                            to_battlefield,
                            enters_tapped,
                            keep_all_revealed: true,
                            optional_keep: false,
                            exile_instead_of_reveal: exile_instead,
                            rest_to_graveyard: false,
                            rest_to_exile: false,
                            rest_to_bottom: false,
                            random_rest_order: false,
                        });
                        i += 2;
                        continue;
                    }
                    if (to_hand || to_battlefield)
                        && (single_card || (keep_all && count_u8 > 1))
                        && i + 2 < ins.len()
                    {
                        match &ins[i + 2] {
                            InstructionAst::MoveZone {
                                subject: rest_subj,
                                to: rest_to,
                            } if matches!(
                                rest_subj.as_ref(),
                                SubjectRefAst::Anaphora(r) if {
                                    let rl = r.to_ascii_lowercase();
                                    rl.contains("rest") || rl.contains("other") || rl == "them" || rl.contains("exiled")
                                }
                            ) =>
                            {
                                let rest_low = rest_to.to_ascii_lowercase();
                                let rest_bottom = rest_low.contains("bottom");
                                let rest_gy = rest_low.contains("graveyard");
                                let rest_exile = rest_low.contains("exile");
                                let random_rest = rest_low.contains("random order");
                                if rest_bottom || rest_gy || rest_exile {
                                    builder.instructions.push(Instruction::RevealUntilMatch {
                                        filter: Box::new(obj_filter),
                                        count: count_u8,
                                        to_battlefield,
                                        enters_tapped,
                                        keep_all_revealed: false,
                                        optional_keep: optional_put,
                                        exile_instead_of_reveal: exile_instead,
                                        rest_to_graveyard: rest_gy,
                                        rest_to_exile: rest_exile,
                                        rest_to_bottom: rest_bottom,
                                        random_rest_order: random_rest,
                                    });
                                    i += 3;
                                    continue;
                                }
                            }
                            InstructionAst::ShuffleLibrary {
                                player: PlayerRefAst::You,
                            } => {
                                builder.instructions.push(Instruction::RevealUntilMatch {
                                    filter: Box::new(obj_filter),
                                    count: count_u8,
                                    to_battlefield,
                                    enters_tapped,
                                    keep_all_revealed: false,
                                    optional_keep: optional_put,
                                    exile_instead_of_reveal: exile_instead,
                                    rest_to_graveyard: false,
                                    rest_to_exile: false,
                                    rest_to_bottom: false,
                                    random_rest_order: false,
                                });
                                builder.instructions.push(Instruction::ShuffleOwnLibrary);
                                i += 3;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        // Consume redundant "reveal it" (LookTopLibrary { reveal: true, library: "ref" }) immediately after a filtered search or revealed look
        if let InstructionAst::LookTopLibrary {
            reveal: true,
            library,
            ..
        } = &ins[i]
            && library == "ref"
            && builder.instructions.last().is_some_and(|prev| {
                matches!(prev, Instruction::SearchLibrary { filter } if filter.reveal)
                    || matches!(prev, Instruction::LookTopChoose { reveal: true, .. })
            })
        {
            i += 1;
            continue;
        }
        // Fuse 2-step LookTopLibrary + ReorderTopLibrary (or MoveZone(them -> top in any order)) -> LookTopChoose(rest_to_top=true, max_picks=0)
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && (matches!(&ins[i + 1], InstructionAst::ReorderTopLibrary)
                || matches!(
                    &ins[i + 1],
                    InstructionAst::MoveZone { subject, to }
                        if matches!(
                            subject.as_ref(),
                            SubjectRefAst::Anaphora(a)
                                if matches!(
                                    a.trim().to_ascii_lowercase().as_str(),
                                    "them" | "those cards" | "the rest" | "the_rest" | "rest"
                                )
                        ) && to.to_ascii_lowercase().contains("top of")
                            && to.to_ascii_lowercase().contains("library")
                ))
        {
            let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            builder.instructions.push(Instruction::LookTopChoose {
                count: cnt,
                library_owner: ResolvingPlayer::Controller,
                filter: None,
                reveal: *reveal,
                reveal_only: false,
                to_battlefield: false,
                enters_tapped: false,
                take_all_matching: false,
                max_picks: 0,
                rest_to_graveyard: false,
                rest_to_exile: false,
                rest_to_top: true,
                random_rest_order: false,
                secondary: None,
            });
            i += 2;
            continue;
        }
        // Fuse mandatory LookTopLibrary(1, reveal: true) + IfCondition(MatchesFilter(Anaphora, f), then_body) (+ optional Otherwise)
        // -> LookTopChoose(reveal_only: true) + BranchIfFalse(ObjectMatches(RevealedTop, f))
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal: true,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && matches!(count.as_ref(), ValueExprAst::Const(1))
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[i + 1]
            && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
            && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
        {
            let mut inner_f = filter.as_ref().clone();
            inner_f.zone = None;
            if let Ok(obj_filter) = lower_object_filter(&inner_f, vocabulary) {
                let else_opt = if i + 2 < ins.len()
                    && let InstructionAst::Otherwise(eb) = &ins[i + 2]
                {
                    Some(eb.as_slice())
                } else {
                    None
                };
                let mut then_b = BodyBuilder::new();
                let then_ok = lower_instruction_seq(
                    then_body,
                    &mut then_b,
                    vocabulary,
                    ids,
                    in_trigger,
                    self_pronoun_valid,
                    bound_x,
                )
                .is_ok()
                    && (then_b.target == BodyTarget::None || then_b.target == builder.target);
                let mut else_b = BodyBuilder::new();
                let else_ok = match else_opt {
                    Some(eb) => {
                        lower_instruction_seq(
                            eb,
                            &mut else_b,
                            vocabulary,
                            ids,
                            in_trigger,
                            self_pronoun_valid,
                            bound_x,
                        )
                        .is_ok()
                            && (else_b.target == BodyTarget::None
                                || else_b.target == builder.target)
                    }
                    None => true,
                };
                if then_ok && else_ok {
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: ValueExpr::literal(1),
                        library_owner: ResolvingPlayer::Controller,
                        filter: None,
                        reveal: true,
                        reveal_only: true,
                        to_battlefield: false,
                        enters_tapped: false,
                        take_all_matching: false,
                        max_picks: 0,
                        rest_to_graveyard: false,
                        rest_to_exile: false,
                        rest_to_top: false,
                        random_rest_order: false,
                        secondary: None,
                    });
                    let branch_idx = builder.instructions.len();
                    builder.instructions.push(Instruction::BranchIfFalse {
                        condition: ResolutionCondition {
                            left: ValueExpr::object_matches(
                                MatchObjectRef::RevealedTop,
                                obj_filter,
                            ),
                            comparison: Comparison::Equal,
                            right: ValueExpr::literal(1),
                        },
                        destination: 0,
                    });
                    let then_shift = isize::try_from(builder.instructions.len())
                        .map_err(|_| "oracle:jump-shift-ovf")?;
                    for mut op in then_b.instructions {
                        op.shift_jumps(then_shift, 0);
                        builder.instructions.push(op);
                    }
                    if else_opt.is_some() {
                        let jump_idx = builder.instructions.len();
                        builder
                            .instructions
                            .push(Instruction::Jump { destination: 0 });
                        let else_start = builder.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            builder.instructions.get_mut(branch_idx)
                        {
                            *destination = else_start;
                        }
                        let else_shift =
                            isize::try_from(else_start).map_err(|_| "oracle:jump-shift-ovf")?;
                        for mut op in else_b.instructions {
                            op.shift_jumps(else_shift, 0);
                            builder.instructions.push(op);
                        }
                        let end_dest = builder.instructions.len();
                        if let Some(Instruction::Jump { destination }) =
                            builder.instructions.get_mut(jump_idx)
                        {
                            *destination = end_dest;
                        }
                    } else {
                        let end_dest = builder.instructions.len();
                        if let Some(Instruction::BranchIfFalse { destination, .. }) =
                            builder.instructions.get_mut(branch_idx)
                        {
                            *destination = end_dest;
                        }
                    }
                    i += if else_opt.is_some() { 3 } else { 2 };
                    continue;
                }
            }
        }
        // Fuse Optional([LookTopLibrary(1, reveal: true), IfCondition(MatchesFilter(Anaphora, f), then_body)])
        // OR [Optional([LookTopLibrary(1, reveal: true)])] + IfCondition(MatchesFilter(Anaphora, f), then_body) (+ optional Otherwise)
        // On permanent abilities uses MayRevealLibraryTop; on spells uses OptionalEffect + LookTopChoose(reveal_only: true).
        if builder.target == BodyTarget::None {
            let use_may_reveal = in_trigger || self_pronoun_valid;
            type RevealTopMatch<'a> = (
                &'a ObjectFilterAst,
                &'a [InstructionAst],
                Option<&'a [InstructionAst]>,
                usize,
            );
            let reveal_top_match: Option<RevealTopMatch<'_>> =
                if let InstructionAst::Optional(inner) = &ins[i]
                    && let [
                        InstructionAst::LookTopLibrary {
                            reveal: true,
                            library,
                            count,
                        },
                        InstructionAst::IfCondition {
                            condition,
                            then_body,
                        },
                    ] = inner.as_slice()
                    && matches!(
                        library.trim().to_ascii_lowercase().as_str(),
                        "your" | "own" | "" | "you"
                    )
                    && matches!(count.as_ref(), ValueExprAst::Const(1))
                    && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
                    && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                {
                    let else_opt = if i + 1 < ins.len()
                        && let InstructionAst::Otherwise(eb) = &ins[i + 1]
                    {
                        Some((eb.as_slice(), 2usize))
                    } else {
                        None
                    };
                    Some((
                        filter.as_ref(),
                        then_body.as_slice(),
                        else_opt.map(|(eb, _)| eb),
                        else_opt.map_or(1, |(_, s)| s),
                    ))
                } else if i + 1 < ins.len() {
                    let is_opt_reveal_one = match &ins[i] {
                        InstructionAst::Optional(inner) => matches!(
                            inner.as_slice(),
                            [InstructionAst::LookTopLibrary {
                                reveal: true,
                                library,
                                count,
                            }] if matches!(
                                library.trim().to_ascii_lowercase().as_str(),
                                "your" | "own" | "" | "you"
                            ) && matches!(count.as_ref(), ValueExprAst::Const(1))
                        ),
                        _ => false,
                    };
                    if is_opt_reveal_one
                        && let InstructionAst::IfCondition {
                            condition,
                            then_body,
                        } = &ins[i + 1]
                        && let ConditionAst::MatchesFilter { subject, filter } = condition.as_ref()
                        && matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
                    {
                        let else_opt = if i + 2 < ins.len()
                            && let InstructionAst::Otherwise(eb) = &ins[i + 2]
                        {
                            Some((eb.as_slice(), 3usize))
                        } else {
                            None
                        };
                        Some((
                            filter.as_ref(),
                            then_body.as_slice(),
                            else_opt.map(|(eb, _)| eb),
                            else_opt.map_or(2, |(_, s)| s),
                        ))
                    } else {
                        None
                    }
                } else {
                    None
                };
            if let Some((filter_ast, then_slice, else_opt, step)) = reveal_top_match {
                let mut inner_f = filter_ast.clone();
                inner_f.zone = None;
                if let Ok(obj_filter) = lower_object_filter(&inner_f, vocabulary) {
                    let mut then_b = BodyBuilder::new();
                    let then_ok = lower_instruction_seq(
                        then_slice,
                        &mut then_b,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )
                    .is_ok()
                        && then_b.target == BodyTarget::None;
                    let mut else_b = BodyBuilder::new();
                    let else_ok = match else_opt {
                        Some(eb) => {
                            lower_instruction_seq(
                                eb,
                                &mut else_b,
                                vocabulary,
                                ids,
                                in_trigger,
                                self_pronoun_valid,
                                bound_x,
                            )
                            .is_ok()
                                && else_b.target == BodyTarget::None
                        }
                        None => true,
                    };
                    if then_ok && else_ok {
                        if use_may_reveal {
                            builder.instructions.push(Instruction::MayRevealLibraryTop);
                        } else {
                            builder.instructions.push(Instruction::OptionalEffect {
                                chooser: ResolvingPlayer::Controller,
                            });
                            builder.instructions.push(Instruction::LookTopChoose {
                                count: ValueExpr::literal(1),
                                library_owner: ResolvingPlayer::Controller,
                                filter: None,
                                reveal: true,
                                reveal_only: true,
                                to_battlefield: false,
                                enters_tapped: false,
                                take_all_matching: false,
                                max_picks: 0,
                                rest_to_graveyard: false,
                                rest_to_exile: false,
                                rest_to_top: false,
                                random_rest_order: false,
                                secondary: None,
                            });
                        }
                        let branch_idx = builder.instructions.len();
                        builder.instructions.push(Instruction::BranchIfFalse {
                            condition: ResolutionCondition {
                                left: ValueExpr::object_matches(
                                    MatchObjectRef::RevealedTop,
                                    obj_filter,
                                ),
                                comparison: Comparison::Equal,
                                right: ValueExpr::literal(1),
                            },
                            destination: 0,
                        });
                        let then_shift = isize::try_from(builder.instructions.len())
                            .map_err(|_| "oracle:jump-shift-ovf")?;
                        for mut op in then_b.instructions {
                            op.shift_jumps(then_shift, 0);
                            builder.instructions.push(op);
                        }
                        if else_opt.is_some() {
                            let jump_idx = builder.instructions.len();
                            builder
                                .instructions
                                .push(Instruction::Jump { destination: 0 });
                            let else_start = builder.instructions.len();
                            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                                builder.instructions.get_mut(branch_idx)
                            {
                                *destination = else_start;
                            }
                            let else_shift =
                                isize::try_from(else_start).map_err(|_| "oracle:jump-shift-ovf")?;
                            for mut op in else_b.instructions {
                                op.shift_jumps(else_shift, 0);
                                builder.instructions.push(op);
                            }
                            let end_dest = builder.instructions.len();
                            if let Some(Instruction::Jump { destination }) =
                                builder.instructions.get_mut(jump_idx)
                            {
                                *destination = end_dest;
                            }
                        } else {
                            let end_dest = builder.instructions.len();
                            if let Some(Instruction::BranchIfFalse { destination, .. }) =
                                builder.instructions.get_mut(branch_idx)
                            {
                                *destination = end_dest;
                            }
                        }
                        i += step;
                        continue;
                    }
                }
            }
        }
        // Unwrap Optional([LookTopLibrary, ...]) where the inner slice lowers cleanly to LookTopChoose (+ optional ShuffleOwnLibrary).
        // Never wrap LookTopOneRestBottom / LookTopOneRestGraveyard in OptionalEffect (program.rs rejects them).
        if let InstructionAst::Optional(inner) = &ins[i]
            && inner.len() >= 2
            && matches!(inner.first(), Some(InstructionAst::LookTopLibrary { .. }))
        {
            let mut sub = BodyBuilder::new();
            if lower_instruction_seq(
                inner,
                &mut sub,
                vocabulary,
                ids,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )
            .is_ok()
                && sub.target == BodyTarget::None
            {
                let converted: Vec<Instruction> = sub
                    .instructions
                    .into_iter()
                    .map(|op| match op {
                        Instruction::LookTopOneRestBottom { count } => Instruction::LookTopChoose {
                            count,
                            library_owner: ResolvingPlayer::Controller,
                            filter: None,
                            reveal: false,
                            reveal_only: false,
                            to_battlefield: false,
                            enters_tapped: false,
                            take_all_matching: false,
                            max_picks: 1,
                            rest_to_graveyard: false,
                            rest_to_exile: false,
                            rest_to_top: false,
                            random_rest_order: false,
                            secondary: None,
                        },
                        Instruction::LookTopOneRestGraveyard { count } => {
                            Instruction::LookTopChoose {
                                count: ValueExpr::literal(i64::from(count)),
                                library_owner: ResolvingPlayer::Controller,
                                filter: None,
                                reveal: false,
                                reveal_only: false,
                                to_battlefield: false,
                                enters_tapped: false,
                                take_all_matching: false,
                                max_picks: 1,
                                rest_to_graveyard: true,
                                rest_to_exile: false,
                                rest_to_top: false,
                                random_rest_order: false,
                                secondary: None,
                            }
                        }
                        other => other,
                    })
                    .collect();
                if matches!(converted.as_slice(), [Instruction::LookTopChoose { .. }]) {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                    builder.instructions.extend(converted);
                    i += 1;
                    continue;
                }
                if matches!(
                    converted.as_slice(),
                    [
                        Instruction::LookTopChoose { .. },
                        Instruction::ShuffleOwnLibrary
                    ]
                ) {
                    builder.instructions.push(Instruction::OptionalSequence {
                        chooser: ResolvingPlayer::Controller,
                        instructions: 2,
                    });
                    builder.instructions.extend(converted);
                    i += 1;
                    continue;
                }
            }
        }
        // Fuse 4-step LookTopLibrary + [Optional?]RevealFromLooked(to="revealed") + [Optional?]MoveZone(it -> hand/bf) + MoveZone(rest -> bottom/gy/exile/top)
        // AND 3-step LookTopLibrary + Optional([RevealFromLooked(to="revealed"), MoveZone(it -> hand/bf)]) + MoveZone(rest -> bottom/gy/exile/top)
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
        {
            let parse_reveal_pick_arity = |phrase: &str, rev_tag: &str| -> (bool, u8) {
                if rev_tag == "revealed:all" {
                    return (true, 1);
                }
                if rev_tag == "revealed:any_number" {
                    return (false, u8::MAX);
                }
                if let Some(num_str) = rev_tag.strip_prefix("revealed:")
                    && let Ok(n) = num_str.parse::<u8>()
                    && n >= 2
                {
                    return (false, n);
                }
                let low = phrase.trim().to_ascii_lowercase();
                if low.contains("all of them")
                    || low.contains("all cards")
                    || low.starts_with("all ")
                    || low.contains("each of them")
                {
                    (true, 1)
                } else if low.contains("any number") {
                    (false, u8::MAX)
                } else if low.contains("four")
                    || low.starts_with("up to 4 ")
                    || low.starts_with("4 ")
                {
                    (false, 4)
                } else if low.contains("three")
                    || low.starts_with("up to 3 ")
                    || low.starts_with("3 ")
                {
                    (false, 3)
                } else if low.contains("two")
                    || low.starts_with("up to 2 ")
                    || low.starts_with("2 ")
                {
                    (false, 2)
                } else {
                    (false, 1)
                }
            };
            let four_step = if i + 3 < ins.len() {
                let rev = match &ins[i + 1] {
                    InstructionAst::RevealFromLooked { filter, to }
                        if to == "revealed" || to.starts_with("revealed:") =>
                    {
                        Some((filter, to.as_str()))
                    }
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::RevealFromLooked { filter, to }]
                            if to == "revealed" || to.starts_with("revealed:") =>
                        {
                            Some((filter, to.as_str()))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let mv = match &ins[i + 2] {
                    InstructionAst::MoveZone { subject, to } => Some((subject.as_ref(), to)),
                    InstructionAst::Optional(inner) => match inner.as_slice() {
                        [InstructionAst::MoveZone { subject, to }] => Some((subject.as_ref(), to)),
                        _ => None,
                    },
                    _ => None,
                };
                let rest = match &ins[i + 3] {
                    InstructionAst::MoveZone { subject, to }
                        if matches!(
                            subject.as_ref(),
                            SubjectRefAst::Anaphora(a) if matches!(
                                a.to_ascii_lowercase().as_str(),
                                "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                            )
                        ) =>
                    {
                        Some(to.as_str())
                    }
                    InstructionAst::ReorderTopLibrary => Some("top of your library"),
                    _ => None,
                };
                match (rev, mv, rest) {
                    (
                        Some((filter_ast, rev_tag)),
                        Some((SubjectRefAst::Anaphora(a), keep_to)),
                        Some(rest_to),
                    ) if matches!(
                        a.to_ascii_lowercase().as_str(),
                        "it" | "that card"
                            | "the card"
                            | "the revealed card"
                            | "them"
                            | "those cards"
                    ) =>
                    {
                        let (all_m, max_p) = parse_reveal_pick_arity(keep_to, rev_tag);
                        Some((filter_ast, keep_to.as_str(), rest_to, all_m, max_p, 4usize))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let three_step_opt = if four_step.is_none() {
                if let InstructionAst::Optional(inner) = &ins[i + 1]
                    && let [
                        InstructionAst::RevealFromLooked { filter, to },
                        InstructionAst::MoveZone {
                            subject,
                            to: keep_to,
                        },
                    ] = inner.as_slice()
                    && (to == "revealed" || to.starts_with("revealed:"))
                    && matches!(
                        subject.as_ref(),
                        SubjectRefAst::Anaphora(a) if matches!(
                            a.to_ascii_lowercase().as_str(),
                            "it" | "that card" | "the card" | "the revealed card" | "them" | "those cards"
                        )
                    )
                {
                    let rest = match &ins[i + 2] {
                        InstructionAst::MoveZone { subject, to }
                            if matches!(
                                subject.as_ref(),
                                SubjectRefAst::Anaphora(a) if matches!(
                                    a.to_ascii_lowercase().as_str(),
                                    "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                                )
                            ) =>
                        {
                            Some(to.as_str())
                        }
                        InstructionAst::ReorderTopLibrary => Some("top of your library"),
                        _ => None,
                    };
                    let (all_m, max_p) = parse_reveal_pick_arity(keep_to, to);
                    rest.map(|r| (filter, keep_to.as_str(), r, all_m, max_p, 3usize))
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((filter_ast, keep_to, rest_to, take_all_matching, max_picks, step)) =
                four_step.or(three_step_opt)
            {
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let rest_top = rest_low.contains("top") && rest_low.contains("library");
                let random_rest = rest_low.contains("random order");
                if (to_hand || to_battlefield)
                    && (rest_bottom || rest_gy || rest_exile || rest_top)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: cnt,
                        library_owner: ResolvingPlayer::Controller,
                        filter: Some(Box::new(obj_filter)),
                        reveal: *reveal,
                        reveal_only: false,
                        to_battlefield,
                        enters_tapped,
                        take_all_matching,
                        max_picks,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                        rest_to_top: rest_top,
                        random_rest_order: random_rest,
                        secondary: None,
                    });
                    i += step;
                    continue;
                }
            }
        }
        // Fuse 3-step LookTopLibrary + [Optional(RevealFromLooked) | RevealFromLooked] + MoveZone(rest -> bottom/graveyard/exile/top) -> LookTopChoose
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && let InstructionAst::MoveZone {
                subject: rest_subj,
                to: rest_to,
            } = &ins[i + 2]
            && matches!(
                rest_subj.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.to_ascii_lowercase().as_str(),
                    "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                )
            )
        {
            let rev_opt = match &ins[i + 1] {
                InstructionAst::RevealFromLooked { filter, to } => Some((filter, to)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [InstructionAst::RevealFromLooked { filter, to }] => Some((filter, to)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((filter_ast, keep_to)) = rev_opt {
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let rest_top = rest_low.contains("top") && rest_low.contains("library");
                let random_rest = rest_low.contains("random order");
                let (take_all_matching, max_picks) = if keep_low.contains("all of them")
                    || keep_low.contains("all cards")
                    || keep_low.starts_with("all ")
                {
                    (true, 1)
                } else if keep_low.contains("any number") {
                    (false, u8::MAX)
                } else if keep_low.contains("four")
                    || keep_low.starts_with("up to 4 ")
                    || keep_low.starts_with("4 ")
                {
                    (false, 4)
                } else if keep_low.contains("three")
                    || keep_low.starts_with("up to 3 ")
                    || keep_low.starts_with("3 ")
                {
                    (false, 3)
                } else if keep_low.contains("two")
                    || keep_low.starts_with("up to 2 ")
                    || keep_low.starts_with("2 ")
                {
                    (false, 2)
                } else {
                    (false, 1)
                };
                if (to_hand || to_battlefield)
                    && (rest_bottom || rest_gy || rest_exile || rest_top)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: cnt,
                        library_owner: ResolvingPlayer::Controller,
                        filter: Some(Box::new(obj_filter)),
                        reveal: *reveal,
                        reveal_only: false,
                        to_battlefield,
                        enters_tapped,
                        take_all_matching,
                        max_picks,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                        rest_to_top: rest_top,
                        random_rest_order: random_rest,
                        secondary: None,
                    });
                    i += 3;
                    continue;
                }
            }
        }
        // Fuse 2-step LookTopLibrary + [Optional?]RevealFromLooked (+ optional ShuffleLibrary) -> LookTopChoose
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.trim().to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
        {
            let rev_opt = match &ins[i + 1] {
                InstructionAst::RevealFromLooked { filter, to } => Some((filter, to)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [InstructionAst::RevealFromLooked { filter, to }] => Some((filter, to)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((filter_ast, keep_to)) = rev_opt {
                let keep_low = keep_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let (take_all_matching, max_picks) = if keep_low.contains("all of them")
                    || keep_low.contains("all cards")
                    || keep_low.starts_with("all ")
                {
                    (true, 1)
                } else if keep_low.contains("any number") {
                    (false, u8::MAX)
                } else if keep_low.contains("four") {
                    (false, 4)
                } else if keep_low.contains("three") {
                    (false, 3)
                } else if keep_low.contains("two") {
                    (false, 2)
                } else {
                    (false, 1)
                };
                if (to_hand || to_battlefield)
                    && let Ok(obj_filter) = lower_object_filter(filter_ast, vocabulary)
                {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    let consume_shuffle = i + 2 < ins.len()
                        && matches!(
                            &ins[i + 2],
                            InstructionAst::ShuffleLibrary {
                                player: PlayerRefAst::You
                            }
                        );
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: cnt,
                        library_owner: ResolvingPlayer::Controller,
                        filter: Some(Box::new(obj_filter)),
                        reveal: *reveal,
                        reveal_only: false,
                        to_battlefield,
                        enters_tapped,
                        take_all_matching,
                        max_picks,
                        rest_to_graveyard: false,
                        rest_to_exile: false,
                        rest_to_top: false,
                        random_rest_order: false,
                        secondary: None,
                    });
                    if consume_shuffle {
                        builder.instructions.push(Instruction::ShuffleOwnLibrary);
                    }
                    i += if consume_shuffle { 3 } else { 2 };
                    continue;
                }
            }
        }
        // Fuse 3-step LookTopLibrary + [Optional?]MoveZone(subject -> hand/bf) + MoveZone(rest -> bottom/gy/exile) -> LookTopChoose
        if i + 2 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && let InstructionAst::MoveZone {
                subject: rest_subj,
                to: rest_to,
            } = &ins[i + 2]
            && matches!(
                rest_subj.as_ref(),
                SubjectRefAst::Anaphora(a) if matches!(
                    a.to_ascii_lowercase().as_str(),
                    "the_rest"
                                    | "the rest"
                                    | "rest"
                                    | "the other cards"
                                    | "the others"
                                    | "them"
                                    | "those cards"
                                    | "the rest of them"
                                    | "the rest of the cards"
                                    | "the remaining cards"
                )
            )
        {
            let mv_opt = match &ins[i + 1] {
                InstructionAst::MoveZone { subject, to } => Some((subject.as_ref(), to)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [InstructionAst::MoveZone { subject, to }] => Some((subject.as_ref(), to)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((keep_subj, keep_to)) = mv_opt {
                let keep_low = keep_to.to_ascii_lowercase();
                let rest_low = rest_to.to_ascii_lowercase();
                let to_hand = keep_low.contains("hand");
                let to_battlefield = keep_low.contains("battlefield");
                let enters_tapped = to_battlefield && keep_low.contains("tapped");
                let rest_bottom = rest_low.contains("bottom");
                let rest_gy = rest_low.contains("graveyard");
                let rest_exile = rest_low.contains("exile");
                let rest_top = rest_low.contains("top") && rest_low.contains("library");
                if (to_hand || to_battlefield) && (rest_bottom || rest_gy || rest_exile || rest_top)
                {
                    let parsed_keep: Option<(bool, Option<ObjectFilter>)> = match keep_subj {
                        SubjectRefAst::Group(f) => {
                            if is_unrestricted_filter(f) {
                                Some((false, None))
                            } else {
                                lower_object_filter(f, vocabulary)
                                    .ok()
                                    .map(|o| (false, Some(o)))
                            }
                        }
                        SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                            if is_unrestricted_filter(f) {
                                Some((true, None))
                            } else {
                                lower_object_filter(f, vocabulary)
                                    .ok()
                                    .map(|o| (true, Some(o)))
                            }
                        }
                        SubjectRefAst::Anaphora(a)
                            if matches!(
                                a.to_ascii_lowercase().as_str(),
                                "one of them" | "one" | "that card" | "it"
                            ) =>
                        {
                            Some((false, None))
                        }
                        _ => None,
                    };
                    if let Some((take_all_matching, maybe_filter)) = parsed_keep {
                        let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                        let random_rest = rest_low.contains("random order");
                        builder.instructions.push(Instruction::LookTopChoose {
                            count: cnt,
                            library_owner: ResolvingPlayer::Controller,
                            filter: maybe_filter.map(Box::new),
                            reveal: *reveal,
                            reveal_only: false,
                            to_battlefield,
                            enters_tapped,
                            take_all_matching,
                            max_picks: 1,
                            rest_to_graveyard: rest_gy,
                            rest_to_exile: rest_exile,
                            rest_to_top: rest_top,
                            random_rest_order: random_rest,
                            secondary: None,
                        });
                        i += 3;
                        continue;
                    }
                }
            }
        }
        // Fuse LookTopLibrary + ChooseAndKeep (e.g. Anticipate, Impulse, Sleight of Hand, Forbidden Alchemy, Commune, Mulch, Collected Company, Dig Through Time)
        if i + 1 < ins.len()
            && let InstructionAst::LookTopLibrary {
                reveal,
                library,
                count,
            } = &ins[i]
            && matches!(
                library.to_ascii_lowercase().as_str(),
                "your" | "own" | "" | "you"
            )
            && let InstructionAst::ChooseAndKeep {
                pick,
                keep_to,
                rest_to,
            } = &ins[i + 1]
        {
            let keep_low = keep_to.to_ascii_lowercase();
            let rest_low = rest_to.to_ascii_lowercase();
            let to_hand = keep_low.contains("hand");
            let to_battlefield = keep_low.contains("battlefield");
            let enters_tapped = to_battlefield && keep_low.contains("tapped");
            let rest_bottom = rest_low.contains("bottom");
            let rest_gy = rest_low.contains("graveyard");
            let rest_exile = rest_low.contains("exile");
            let rest_top = rest_low.contains("top") && rest_low.contains("library");
            let pick_low = pick.trim().to_ascii_lowercase();
            if (to_hand || to_battlefield) && (rest_bottom || rest_gy || rest_exile || rest_top) {
                let any_one = matches!(
                    pick_low.as_str(),
                    "one"
                        | "1"
                        | "one of them"
                        | "one of those cards"
                        | "a card from among them"
                        | "that card"
                        | "a card"
                        | "any one"
                );
                let any_multi: Option<u8> = match pick_low.as_str() {
                    "two"
                    | "2"
                    | "two of them"
                    | "up to two"
                    | "up to two of them"
                    | "two of those cards"
                    | "up to two cards from among them" => Some(2),
                    "three" | "3" | "three of them" | "up to three" | "up to three of them" => {
                        Some(3)
                    }
                    "four" | "4" | "four of them" | "up to four" | "up to four of them" => Some(4),
                    _ => None,
                };
                let (take_all_matching, max_picks, filter_opt): (
                    bool,
                    u8,
                    Option<Option<ObjectFilter>>,
                ) = if any_one {
                    (false, 1, Some(None))
                } else if let Some(k) = any_multi {
                    (false, k, Some(None))
                } else if let Some(rest) = pick_low
                    .strip_prefix("all ")
                    .or_else(|| pick_low.strip_prefix("each "))
                {
                    let cleaned = rest
                        .trim_end_matches(" revealed this way")
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    (
                        true,
                        1,
                        parse_look_top_pick_filter(cleaned, vocabulary).map(Some),
                    )
                } else if let Some((k, rest)) = pick_low
                    .strip_prefix("up to two ")
                    .or_else(|| pick_low.strip_prefix("two "))
                    .map(|r| (2u8, r))
                    .or_else(|| {
                        pick_low
                            .strip_prefix("up to three ")
                            .or_else(|| pick_low.strip_prefix("three "))
                            .map(|r| (3u8, r))
                    })
                    .or_else(|| {
                        pick_low
                            .strip_prefix("up to four ")
                            .or_else(|| pick_low.strip_prefix("four "))
                            .map(|r| (4u8, r))
                    })
                    .or_else(|| {
                        pick_low
                            .strip_prefix("any number of ")
                            .map(|r| (u8::MAX, r))
                    })
                {
                    let cleaned = rest
                        .trim_end_matches(" revealed this way")
                        .trim_end_matches(" from among them")
                        .trim_end_matches(" cards")
                        .trim_end_matches(" card")
                        .trim();
                    (
                        false,
                        k,
                        parse_look_top_pick_filter(cleaned, vocabulary).map(Some),
                    )
                } else {
                    (
                        false,
                        1,
                        parse_look_top_pick_filter(&pick_low, vocabulary).map(Some),
                    )
                };
                if let Some(maybe_filter) = filter_opt {
                    let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                    if !*reveal
                        && !take_all_matching
                        && max_picks == 1
                        && maybe_filter.is_none()
                        && to_hand
                        && rest_bottom
                    {
                        builder
                            .instructions
                            .push(Instruction::LookTopOneRestBottom { count: cnt });
                        i += 2;
                        continue;
                    }
                    if !*reveal
                        && !take_all_matching
                        && max_picks == 1
                        && maybe_filter.is_none()
                        && to_hand
                        && rest_gy
                        && let ValueExpr::Literal(n) = &cnt
                        && let Some(u) = n.to_usize().and_then(|v| u8::try_from(v).ok())
                        && u > 0
                    {
                        builder
                            .instructions
                            .push(Instruction::LookTopOneRestGraveyard { count: u });
                        i += 2;
                        continue;
                    }
                    let random_rest = rest_low.contains("random order");
                    builder.instructions.push(Instruction::LookTopChoose {
                        count: cnt,
                        library_owner: ResolvingPlayer::Controller,
                        filter: maybe_filter.map(Box::new),
                        reveal: *reveal,
                        reveal_only: false,
                        to_battlefield,
                        enters_tapped,
                        take_all_matching,
                        max_picks,
                        rest_to_graveyard: rest_gy,
                        rest_to_exile: rest_exile,
                        rest_to_top: rest_top,
                        random_rest_order: random_rest,
                        secondary: None,
                    });
                    i += 2;
                    continue;
                }
            }
        }
        // Fold trailing "you may spend mana as though it were mana of any color/type" onto prior GrantCardPlayPermission / ExileTopPlay
        if let InstructionAst::ManaSpendRestriction(r) = &ins[i] {
            let rl = r.trim().to_ascii_lowercase();
            if rl.contains("any color") || rl.contains("any type") {
                let mut patched = false;
                for op in builder.instructions.iter_mut().rev() {
                    match op {
                        Instruction::GrantCardPlayPermission { any_color, .. }
                        | Instruction::ExileTopPlay { any_color, .. } => {
                            *any_color = true;
                            patched = true;
                            break;
                        }
                        _ => {}
                    }
                }
                if patched {
                    i += 1;
                    continue;
                }
            }
        }
        // Fold LookTopLibrary("exiled" | "ref") after prior exile/draw/look/search
        if let InstructionAst::LookTopLibrary { library, .. } = &ins[i] {
            let ll = library.trim().to_ascii_lowercase();
            if (ll == "exiled"
                && builder.instructions.iter().rev().any(|op| {
                    matches!(
                        op,
                        Instruction::ExileTopPlay { .. }
                            | Instruction::ExileTopForPlay { .. }
                            | Instruction::ExileLibraryTop { .. }
                    )
                }))
                || (ll == "ref"
                    && builder.instructions.iter().rev().any(|op| {
                        matches!(
                            op,
                            Instruction::LookTopChoose { .. }
                                | Instruction::SearchLibrary { .. }
                                | Instruction::DrawController { .. }
                                | Instruction::DrawPlayer { .. }
                                | Instruction::ExileLibraryTop { .. }
                        )
                    }))
            {
                i += 1;
                continue;
            }
        }
        // Fuse multi-sentence SearchLibrary(shuffle: false, to_zone: "found_set"|...) + [LookTop("ref")?] + [MoveZone/ReturnZone(Anaphora)?] + ShuffleLibrary
        if let InstructionAst::SearchLibrary {
            library,
            count,
            filter,
            reveal,
            to_zone,
            shuffle: false,
            up_to,
            any_number,
        } = &ins[i]
        {
            let mut cursor = i + 1;
            let mut eff_reveal = *reveal;
            let mut eff_dest = to_zone.clone();
            if cursor < ins.len()
                && let InstructionAst::LookTopLibrary {
                    reveal: r2,
                    library: l2,
                    ..
                } = &ins[cursor]
                && l2.eq_ignore_ascii_case("ref")
            {
                eff_reveal |= *r2;
                cursor += 1;
            }
            if cursor < ins.len() {
                match &ins[cursor] {
                    InstructionAst::MoveZone { subject, to }
                        if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) =>
                    {
                        eff_dest = to.clone();
                        cursor += 1;
                    }
                    InstructionAst::ReturnZone {
                        subject,
                        to,
                        tapped,
                        transformed: false,
                        with_counter: None,
                        attached_to: None,
                        ..
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                        eff_dest = if *tapped && !to.to_ascii_lowercase().contains("tapped") {
                            format!("{to} tapped")
                        } else {
                            to.clone()
                        };
                        cursor += 1;
                    }
                    _ => {}
                }
            }
            if cursor < ins.len()
                && matches!(
                    &ins[cursor],
                    InstructionAst::ShuffleLibrary {
                        player: PlayerRefAst::You | PlayerRefAst::ThatPlayer
                    }
                )
                && let Ok(search) = lower_library_search(
                    library,
                    count,
                    filter,
                    eff_reveal,
                    &eff_dest,
                    vocabulary,
                    false,
                    *up_to,
                    *any_number,
                )
            {
                if search.source == SearchSource::TargetOpponentLibrary {
                    builder.bind_target(BodyTarget::SingleOpponent)?;
                } else if search.searcher == ResolvingPlayer::TargetPlayer {
                    builder.bind_target(BodyTarget::SinglePlayer)?;
                }
                builder.instructions.push(Instruction::SearchLibrary {
                    filter: Box::new(search),
                });
                i = cursor + 1;
                continue;
            }
        }
        // Standalone LookTopLibrary(N, reveal: true|false) -> LookTopChoose(reveal_only: true)
        if let InstructionAst::LookTopLibrary {
            reveal,
            library,
            count,
        } = &ins[i]
        {
            let lib_low = library.trim().to_ascii_lowercase();
            let owner_opt = match lib_low.as_str() {
                "your" | "own" | "" | "you" | "yourlibrary" | "ownlibrary" => {
                    Some((None, ResolvingPlayer::Controller))
                }
                "target player" | "target player's" | "targetplayer" => Some((
                    Some(BodyTarget::SinglePlayer),
                    ResolvingPlayer::TargetPlayer,
                )),
                "target opponent" | "target opponent's" | "targetopponent" => Some((
                    Some(BodyTarget::SingleOpponent),
                    ResolvingPlayer::TargetPlayer,
                )),
                "target"
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    Some((None, ResolvingPlayer::TargetPlayer))
                }
                "their" | "their library"
                    if matches!(
                        builder.target,
                        BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
                    ) =>
                {
                    Some((None, ResolvingPlayer::TargetPlayer))
                }
                "their" | "their library" => Some((None, ResolvingPlayer::UniqueRelatedPlayer)),
                "that player" | "that player's" | "thatplayer" | "its controller"
                | "its controller's" | "itscontroller"
                    if in_trigger || builder.target == BodyTarget::None =>
                {
                    Some((None, ResolvingPlayer::UniqueRelatedPlayer))
                }
                "its controller" | "its controller's" | "itscontroller"
                    if builder.target != BodyTarget::None =>
                {
                    Some((None, ResolvingPlayer::TargetController))
                }
                "its owner" | "its owner's" | "itsowner" if builder.target != BodyTarget::None => {
                    Some((None, ResolvingPlayer::TargetOwner))
                }
                "defending player" | "defending player's" | "defendingplayer" => {
                    Some((None, ResolvingPlayer::DefendingPlayer))
                }
                "enchanted player" | "enchanted player's" | "enchantedplayer" => {
                    Some((None, ResolvingPlayer::EnchantedPlayer))
                }
                "active player" | "active player's" | "activeplayer" => {
                    Some((None, ResolvingPlayer::ActivePlayer))
                }
                _ => None,
            };
            if let Some((req_bt, library_owner)) = owner_opt {
                if let Some(bt) = req_bt {
                    builder.bind_target(bt)?;
                }
                let cnt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
                builder.instructions.push(Instruction::LookTopChoose {
                    count: cnt,
                    library_owner,
                    filter: None,
                    reveal: *reveal,
                    reveal_only: true,
                    to_battlefield: false,
                    enters_tapped: false,
                    take_all_matching: false,
                    max_picks: 0,
                    rest_to_graveyard: false,
                    rest_to_exile: false,
                    rest_to_top: false,
                    random_rest_order: false,
                    secondary: None,
                });
                i += 1;
                continue;
            }
        }
        // Fuse RevealHand / LookAtHand (target opponent/player) + optional ChooseOption("<filter> from it")
        // + Discard / Exile(chosen card) -> ChooseRevealedHandCard
        let hand_reveal_player = match &ins[i] {
            InstructionAst::RevealHand {
                player: p @ (PlayerRefAst::TargetOpponent | PlayerRefAst::TargetPlayer),
            }
            | InstructionAst::LookAtHand {
                player: p @ (PlayerRefAst::TargetOpponent | PlayerRefAst::TargetPlayer),
            } => Some(*p),
            _ => None,
        };
        if let Some(reveal_player) = hand_reveal_player {
            let mut fused_step = 0usize;
            let mut chosen_spec: Option<(u8, Option<ObjectFilterAst>, ChosenHandAction)> = None;
            // 3-step pattern: RevealHand/LookAtHand + ChooseOption("<count?> <filter> from it/them") + Discard/Exile(anaphora)
            if i + 2 < ins.len()
                && let InstructionAst::ChooseOption(opt_desc) = &ins[i + 1]
                && let Some(core_desc) = opt_desc
                    .trim()
                    .strip_suffix(" from it")
                    .or_else(|| opt_desc.trim().strip_suffix(" from them"))
            {
                let action_opt = match &ins[i + 2] {
                    InstructionAst::Discard {
                        player:
                            PlayerRefAst::ThatPlayer
                            | PlayerRefAst::TargetOpponent
                            | PlayerRefAst::TargetPlayer,
                        count,
                        filter: None,
                        anaphora: _,
                        random: false,
                    } if count.is_none()
                        || matches!(count.as_deref(), Some(ValueExprAst::Const(1))) =>
                    {
                        Some(ChosenHandAction::Discard)
                    }
                    InstructionAst::Exile {
                        actor: None | Some(PlayerRefAst::You),
                        subject,
                    } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_)) => {
                        Some(ChosenHandAction::Exile)
                    }
                    _ => None,
                };
                if let Some(action) = action_opt {
                    let core = core_desc.trim();
                    let (pick_n, flt_raw) = if let Some(r) = core.strip_prefix("two ") {
                        (2u8, format!("a {r}"))
                    } else if let Some(r) = core.strip_prefix("2 ") {
                        (2u8, format!("a {r}"))
                    } else if let Some(r) = core.strip_prefix("three ") {
                        (3u8, format!("a {r}"))
                    } else {
                        (1u8, core.to_string())
                    };
                    if let Ok(ast_f) = spellwake_oracle_compiler::filter::parse_object_filter(
                        &flt_raw,
                        &cached_compiler_vocab(vocabulary),
                    ) {
                        chosen_spec = Some((pick_n, Some(ast_f), action));
                        fused_step = 3;
                    }
                }
            }
            // 2-step pattern: RevealHand/LookAtHand + Discard/Exile(chosen card)
            if chosen_spec.is_none() && i + 1 < ins.len() {
                match &ins[i + 1] {
                    InstructionAst::Discard {
                        player:
                            PlayerRefAst::ThatPlayer
                            | PlayerRefAst::TargetOpponent
                            | PlayerRefAst::TargetPlayer,
                        count,
                        filter,
                        anaphora: _,
                        random: false,
                    } => {
                        let cnt_u8: Option<u8> = match count.as_deref() {
                            None => Some(1),
                            Some(ValueExprAst::Const(n)) if *n > 0 => u8::try_from(*n).ok(),
                            _ => None,
                        };
                        if let Some(n) = cnt_u8 {
                            chosen_spec =
                                Some((n, filter.as_deref().cloned(), ChosenHandAction::Discard));
                            fused_step = 2;
                        }
                    }
                    InstructionAst::Exile {
                        actor: None | Some(PlayerRefAst::You),
                        subject,
                    } => match subject.as_ref() {
                        SubjectRefAst::Anaphora(_) => {
                            chosen_spec = Some((1, None, ChosenHandAction::Exile));
                            fused_step = 2;
                        }
                        SubjectRefAst::Group(f)
                            if matches!(f.zone, None | Some(ZoneFilter::ThatPlayerHand)) =>
                        {
                            chosen_spec =
                                Some((1, Some(f.as_ref().clone()), ChosenHandAction::Exile));
                            fused_step = 2;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            if let Some((n, filter_opt, action)) = chosen_spec
                && let Some(nz) = NonZeroU8::new(n)
            {
                let lowered_filter: Result<(bool, u16, Option<Box<ObjectFilter>>), &'static str> =
                    match filter_opt {
                        None => Ok((false, 0, None)),
                        Some(mut f) => {
                            if !matches!(f.zone, None | Some(ZoneFilter::ThatPlayerHand))
                                || f.controller.is_some()
                                || f.named.is_some()
                            {
                                Err("oracle:unsupported-revealed-hand-filter")
                            } else {
                                f.zone = None;
                                if is_unrestricted_filter(&f) {
                                    Ok((false, 0, None))
                                } else {
                                    lower_object_filter(&f, vocabulary)
                                        .map(|obj| (false, 0, Some(Box::new(obj))))
                                }
                            }
                        }
                    };
                if let Ok((nonland_only, minimum_mana_value, filter)) = lowered_filter {
                    builder.bind_target(if reveal_player == PlayerRefAst::TargetOpponent {
                        BodyTarget::SingleOpponent
                    } else {
                        BodyTarget::SinglePlayer
                    })?;
                    builder
                        .instructions
                        .push(Instruction::ChooseRevealedHandCard {
                            minimum_mana_value,
                            nonland_only,
                            filter,
                            count: nz,
                            action,
                        });
                    i += fused_step;
                    continue;
                }
            }
        }
        // Fuse AddMana (or BindX([AddMana])) + [optional 1 side-effect] + ManaSpendRestriction
        let mana_fuse = match &ins[i] {
            InstructionAst::AddMana {
                player: PlayerRefAst::You,
                mana,
            } => Some((mana, bound_x)),
            InstructionAst::BindX { x_expr, body } => match body.as_slice() {
                [
                    InstructionAst::AddMana {
                        player: PlayerRefAst::You,
                        mana,
                    },
                ] => Some((mana, Some(x_expr.as_ref()))),
                _ => None,
            },
            _ => None,
        };
        let mana_restr_lookahead = if mana_fuse.is_some() {
            if let Some(InstructionAst::ManaSpendRestriction(r)) = ins.get(i + 1) {
                parse_mana_restriction(r, vocabulary).map(|res| (res, 2usize, None))
            } else if let Some(InstructionAst::ManaSpendRestriction(r)) = ins.get(i + 2)
                && !matches!(
                    ins.get(i + 1),
                    Some(InstructionAst::AddMana { .. } | InstructionAst::ManaSpendRestriction(_))
                )
            {
                parse_mana_restriction(r, vocabulary).map(|res| (res, 3usize, ins.get(i + 1)))
            } else {
                None
            }
        } else {
            None
        };
        if let (Some((mana, eff_bound_x)), Some((restriction, step_len, intervening))) =
            (mana_fuse, mana_restr_lookahead)
        {
            let mut fused_mana = false;
            match mana {
                ManaSpecAst::Fixed(raw) => {
                    if let Some(bundle) = parse_mana_bundle(raw) {
                        builder.instructions.push(Instruction::AddMana(
                            EffectManaYield::RestrictedFixed {
                                bundle,
                                restriction,
                            },
                        ));
                        fused_mana = true;
                    }
                }
                ManaSpecAst::Choice(opts) if opts.len() >= 2 => {
                    let all_single: Option<BTreeSet<ManaType>> =
                        opts.iter().map(|opt| parse_single_mana_type(opt)).collect();
                    if let Some(colors) = all_single
                        && !colors.is_empty()
                        && !colors.contains(&ManaType::Colourless)
                    {
                        builder.instructions.push(Instruction::AddMana(
                            EffectManaYield::RestrictedChoice {
                                colors,
                                amount: ValueExpr::literal(1),
                                restriction,
                            },
                        ));
                        fused_mana = true;
                    } else if let Some(bundles) = opts
                        .iter()
                        .map(|opt| parse_mana_bundle(opt))
                        .collect::<Option<Vec<_>>>()
                        && let Some(first) = bundles.first()
                        && first.len() == 1
                        && let Some(qty) = first.values().next().cloned()
                    {
                        let mut colors = BTreeSet::new();
                        let mut ok = true;
                        for b in &bundles {
                            if b.len() != 1 || b.values().next() != Some(&qty) {
                                ok = false;
                                break;
                            }
                            if let Some(&mt) = b.keys().next() {
                                if mt == ManaType::Colourless {
                                    ok = false;
                                    break;
                                }
                                colors.insert(mt);
                            }
                        }
                        if ok && !colors.is_empty() {
                            builder.instructions.push(Instruction::AddMana(
                                EffectManaYield::RestrictedChoice {
                                    colors,
                                    amount: ValueExpr::Literal(qty),
                                    restriction,
                                },
                            ));
                            fused_mana = true;
                        }
                    }
                }
                ManaSpecAst::AnyColor {
                    amount,
                    single_color,
                } => {
                    if let Ok(amt) = lower_value_expr(resolve_x(amount, eff_bound_x), vocabulary) {
                        let all_colors = BTreeSet::from([
                            ManaType::White,
                            ManaType::Blue,
                            ManaType::Black,
                            ManaType::Red,
                            ManaType::Green,
                        ]);
                        if *single_color {
                            builder.instructions.push(Instruction::AddMana(
                                EffectManaYield::RestrictedChoice {
                                    colors: all_colors,
                                    amount: amt,
                                    restriction,
                                },
                            ));
                        } else {
                            builder.instructions.push(Instruction::RepeatLoop {
                                count: amt,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![Instruction::AddMana(EffectManaYield::RestrictedChoice {
                                        colors: all_colors,
                                        amount: ValueExpr::literal(1),
                                        restriction,
                                    })],
                                )),
                            });
                        }
                        fused_mana = true;
                    }
                }
                ManaSpecAst::CombinationOf { amount, symbols } if symbols.len() >= 2 => {
                    let all_single: Option<BTreeSet<ManaType>> = symbols
                        .iter()
                        .map(|opt| parse_single_mana_type(opt))
                        .collect();
                    if let Some(colors) = all_single
                        && !colors.is_empty()
                        && !colors.contains(&ManaType::Colourless)
                        && let Ok(amt) =
                            lower_value_expr(resolve_x(amount, eff_bound_x), vocabulary)
                    {
                        if amt == ValueExpr::literal(1) {
                            builder.instructions.push(Instruction::AddMana(
                                EffectManaYield::RestrictedChoice {
                                    colors,
                                    amount: ValueExpr::literal(1),
                                    restriction,
                                },
                            ));
                        } else {
                            builder.instructions.push(Instruction::RepeatLoop {
                                count: amt,
                                body: Box::new(ExecutableBody::new(
                                    BodyTarget::None,
                                    vec![Instruction::AddMana(EffectManaYield::RestrictedChoice {
                                        colors,
                                        amount: ValueExpr::literal(1),
                                        restriction,
                                    })],
                                )),
                            });
                        }
                        fused_mana = true;
                    }
                }
                _ => {}
            }
            if fused_mana {
                if let Some(mid) = intervening {
                    lower_instruction(
                        mid,
                        builder,
                        vocabulary,
                        ids,
                        in_trigger,
                        self_pronoun_valid,
                        bound_x,
                    )?;
                }
                i += step_len;
                continue;
            }
        }
        // Fuse CreateToken (1 untapped non-attacking controlled token) + Attach(self -> it) -> CreateTokenAndAttach
        if i + 1 < ins.len()
            && let InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                token,
            } = &ins[i]
            && let InstructionAst::Attach { subject, target } = &ins[i + 1]
            && is_self_pronoun(subject, builder, self_pronoun_valid)
            && matches!(target.as_ref(), SubjectRefAst::Anaphora(_))
            && let Ok((creation, false)) =
                lower_token_spec(token, TokenRecipients::Controller, vocabulary, ids, bound_x)
            && !creation.enters_tapped
            && creation.amount == ValueExpr::literal(1)
            && creation.base_size.is_none()
        {
            builder
                .instructions
                .push(Instruction::CreateTokenAndAttach {
                    definition: Box::new(creation.definition),
                });
            i += 2;
            continue;
        }
        // Fuse bite spells: "Target creature ... deals damage equal to its power to target creature/planeswalker/permanent."
        // (+ optional prefix instructions on target A like "+1/+1", and optional trample excess damage)
        if let InstructionAst::DealDamage {
            source,
            target,
            amount,
        } = &ins[i]
            && matches!(
                amount.as_ref(),
                ValueExprAst::ObjectStat {
                    subject,
                    stat: StatKind::Power,
                } if matches!(subject.as_ref(), SubjectRefAst::Anaphora(_))
            )
            && let SubjectRefAst::TargetObject {
                filter: dst_f,
                count: TargetCountAst::Exact(1),
            } = target.as_ref()
            && dst_f.zone.is_none()
            && let Ok(dst_bt) = lower_battlefield_body_target(dst_f, vocabulary)
            && matches!(
                dst_bt,
                BodyTarget::SingleCreature
                    | BodyTarget::SinglePlaneswalker
                    | BodyTarget::SinglePermanent
                    | BodyTarget::FilteredPermanent(_)
            )
        {
            let trample_overflow = i + 1 < ins.len()
                && matches!(
                    &ins[i + 1],
                    InstructionAst::AbilityRestriction {
                        restriction,
                        condition: None,
                    } if restriction == "ExcessDamageToController"
                );
            if let SubjectRefAst::TargetObject {
                filter: src_f,
                count: TargetCountAst::Exact(1),
            } = source.as_ref()
                && src_f.zone.is_none()
                && let Ok(src_bt) = lower_battlefield_body_target(src_f, vocabulary)
                && src_bt.guarantees_creature()
                && builder.target == BodyTarget::None
                && builder.instructions.is_empty()
            {
                let fixed = |bt: BodyTarget| {
                    Instruction::FixedTargetClause(Box::new(ExecutableBody::new(
                        bt,
                        vec![Instruction::RememberPermanentTarget],
                    )))
                };
                builder.instructions.push(fixed(src_bt));
                builder.instructions.push(fixed(dst_bt));
                builder
                    .instructions
                    .push(Instruction::CreatureDealsPowerDamageToOther { trample_overflow });
                i += if trample_overflow { 2 } else { 1 };
                continue;
            }
            if matches!(source.as_ref(), SubjectRefAst::Anaphora(_))
                && builder.target.guarantees_creature()
                && !builder.instructions.is_empty()
                && builder.graveyard_target_bounds.is_none()
                && !builder.x_target_count
                && !builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::FixedTargetClause(_)
                            | Instruction::AffectPermanentTargets { .. }
                            | Instruction::RememberPermanentTarget
                    )
                })
            {
                let mut first_ops = std::mem::take(&mut builder.instructions);
                prepend_instruction_at(&mut first_ops, 0, Instruction::RememberPermanentTarget);
                let first_bt = std::mem::replace(&mut builder.target, BodyTarget::None);
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(first_bt, first_ops),
                    )));
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(dst_bt, vec![Instruction::RememberPermanentTarget]),
                    )));
                builder
                    .instructions
                    .push(Instruction::CreatureDealsPowerDamageToOther { trample_overflow });
                i += if trample_overflow { 2 } else { 1 };
                continue;
            }
        }
        // Fuse Copy (+ Optional(ChangeTargets) / MayChooseNewTargetsForCopy / AbilityRestriction)
        // directly into Instruction::CopyStackObject { target, count, may_choose_new_targets: true }.
        if i + 1 < ins.len() && is_choose_new_targets_for_copy(&ins[i + 1]) {
            let copy_parts = match &ins[i] {
                InstructionAst::Copy {
                    subject,
                    exception: None,
                    count,
                } => Some((false, subject, count)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [
                        InstructionAst::Copy {
                            subject,
                            exception: None,
                            count,
                        },
                    ] => Some((true, subject, count)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((is_optional, subject, count)) = copy_parts {
                let mut probe = builder.clone();
                if let Ok(count_expr) = match count {
                    Some(c) => lower_value_expr(resolve_x(c, bound_x), vocabulary),
                    None => Ok(ValueExpr::literal(1)),
                } && let Ok(stack_copy_target) = resolve_stack_copy_target(
                    subject,
                    &mut probe,
                    vocabulary,
                    in_trigger,
                    self_pronoun_valid,
                ) {
                    *builder = probe;
                    if is_optional {
                        builder.instructions.push(Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        });
                    }
                    builder.instructions.push(Instruction::CopyStackObject {
                        target: stack_copy_target,
                        count: count_expr,
                        may_choose_new_targets: true,
                    });
                    i += 2;
                    continue;
                }
            }
        }
        // Fuse 2-target Exchange(Control) (direct or Optional) into FixedTargetClause pair + Exchange(Control).
        // Triggered abilities reject FixedTargetClause in Program::validate, so guard on !in_trigger.
        if !in_trigger && builder.target == BodyTarget::None && builder.instructions.is_empty() {
            let exch_parts = match &ins[i] {
                InstructionAst::Exchange {
                    kind: spellwake_oracle_compiler::ast::ExchangeKind::Control,
                    left,
                    right,
                } => Some((false, left, right)),
                InstructionAst::Optional(inner) => match inner.as_slice() {
                    [
                        InstructionAst::Exchange {
                            kind: spellwake_oracle_compiler::ast::ExchangeKind::Control,
                            left,
                            right,
                        },
                    ] => Some((true, left, right)),
                    _ => None,
                },
                _ => None,
            };
            if let Some((is_optional, left, right)) = exch_parts {
                let targets_pair = match (left.as_ref(), right.as_deref()) {
                    (
                        SubjectRefAst::TargetObject {
                            filter: fa,
                            count: TargetCountAst::Exact(1),
                        },
                        Some(SubjectRefAst::TargetObject {
                            filter: fb,
                            count: TargetCountAst::Exact(1),
                        }),
                    ) if fa.zone.is_none() && fb.zone.is_none() => {
                        match (
                            lower_battlefield_body_target(fa, vocabulary),
                            lower_battlefield_body_target(fb, vocabulary),
                        ) {
                            (Ok(bt_a), Ok(bt_b)) => Some((bt_a, bt_b)),
                            _ => None,
                        }
                    }
                    (
                        SubjectRefAst::TargetObject {
                            filter: fa,
                            count: TargetCountAst::Exact(2),
                        },
                        None,
                    ) if fa.zone.is_none() => lower_battlefield_body_target(fa, vocabulary)
                        .ok()
                        .map(|bt| (bt, bt)),
                    (SubjectRefAst::Compound(items), None) if items.len() == 2 => {
                        match (&items[0], &items[1]) {
                            (
                                SubjectRefAst::TargetObject {
                                    filter: fa,
                                    count: TargetCountAst::Exact(1),
                                },
                                SubjectRefAst::TargetObject {
                                    filter: fb,
                                    count: TargetCountAst::Exact(1),
                                },
                            ) if fa.zone.is_none() && fb.zone.is_none() => {
                                match (
                                    lower_battlefield_body_target(fa, vocabulary),
                                    lower_battlefield_body_target(fb, vocabulary),
                                ) {
                                    (Ok(bt_a), Ok(bt_b)) => Some((bt_a, bt_b)),
                                    _ => None,
                                }
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((bt_a, bt_b)) = targets_pair {
                    builder
                        .instructions
                        .push(Instruction::FixedTargetClause(Box::new(
                            ExecutableBody::new(bt_a, vec![Instruction::RememberPermanentTarget]),
                        )));
                    builder
                        .instructions
                        .push(Instruction::FixedTargetClause(Box::new(
                            ExecutableBody::new(bt_b, vec![Instruction::RememberPermanentTarget]),
                        )));
                    if is_optional {
                        builder.instructions.push(Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        });
                    }
                    builder.instructions.push(Instruction::Exchange(
                        spellwake_engine::program::ExchangeSpec::Control {
                            first: Box::new(MoveSubjectRef::RememberedFirst),
                            second: Box::new(MoveSubjectRef::RememberedSecond),
                        },
                    ));
                    i += 1;
                    continue;
                }
            }
        }
        // Fuse 2-target Fight, prefix-on-target-A + Fight, Optional([Fight]), and hybrid trigger/attached Fight
        let fight_parts = match &ins[i] {
            InstructionAst::Fight { a, b } => Some((false, a, b)),
            InstructionAst::Optional(inner) => match inner.as_slice() {
                [InstructionAst::Fight { a, b }] => Some((true, a, b)),
                _ => None,
            },
            _ => None,
        };
        if let Some((is_optional_fight, a, b)) = fight_parts {
            if !in_trigger
                && let (
                    SubjectRefAst::TargetObject {
                        filter: fa,
                        count: TargetCountAst::Exact(1),
                    },
                    SubjectRefAst::TargetObject {
                        filter: fb,
                        count: TargetCountAst::Exact(1),
                    },
                ) = (a.as_ref(), b.as_ref())
                && fa.zone.is_none()
                && fb.zone.is_none()
                && builder.target == BodyTarget::None
                && builder.instructions.is_empty()
                && let Ok(bt_a) = lower_battlefield_body_target(fa, vocabulary)
                && let Ok(bt_b) = lower_battlefield_body_target(fb, vocabulary)
                && bt_a.guarantees_creature()
                && bt_b.guarantees_creature()
            {
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(bt_a, vec![Instruction::RememberPermanentTarget]),
                    )));
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(bt_b, vec![Instruction::RememberPermanentTarget]),
                    )));
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                    builder.instructions.push(Instruction::Fight {
                        first: Box::new(MoveSubjectRef::RememberedFirst),
                        second: Box::new(MoveSubjectRef::RememberedSecond),
                    });
                } else {
                    builder.instructions.push(Instruction::FightFixedTargets);
                }
                i += 1;
                continue;
            }
            let other_target = match (a.as_ref(), b.as_ref()) {
                (
                    SubjectRefAst::Anaphora(_),
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                )
                | (
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                    SubjectRefAst::Anaphora(_),
                ) if filter.zone.is_none() => Some(filter),
                _ => None,
            };
            if !in_trigger
                && let Some(fb) = other_target
                && builder.target.guarantees_creature()
                && !builder.instructions.is_empty()
                && builder.graveyard_target_bounds.is_none()
                && !builder.x_target_count
                && !builder.instructions.iter().any(|op| {
                    matches!(
                        op,
                        Instruction::FixedTargetClause(_)
                            | Instruction::AffectPermanentTargets { .. }
                            | Instruction::RememberPermanentTarget
                    )
                })
                && let Ok(bt_b) = lower_battlefield_body_target(fb, vocabulary)
                && bt_b.guarantees_creature()
            {
                let mut first_ops = std::mem::take(&mut builder.instructions);
                prepend_instruction_at(&mut first_ops, 0, Instruction::RememberPermanentTarget);
                let first_bt = std::mem::replace(&mut builder.target, BodyTarget::None);
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(first_bt, first_ops),
                    )));
                builder
                    .instructions
                    .push(Instruction::FixedTargetClause(Box::new(
                        ExecutableBody::new(bt_b, vec![Instruction::RememberPermanentTarget]),
                    )));
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                    builder.instructions.push(Instruction::Fight {
                        first: Box::new(MoveSubjectRef::RememberedFirst),
                        second: Box::new(MoveSubjectRef::RememberedSecond),
                    });
                } else {
                    builder.instructions.push(Instruction::FightFixedTargets);
                }
                i += 1;
                continue;
            }
            // Hybrid 1-target Fight (triggering creature / attached host vs target creature,
            // or ability source vs already-targeted creature via anaphora).
            let hybrid_target_pair = match (a.as_ref(), b.as_ref()) {
                (
                    lhs,
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                )
                | (
                    SubjectRefAst::TargetObject {
                        filter,
                        count: TargetCountAst::Exact(1),
                    },
                    lhs,
                ) if filter.zone.is_none()
                    && !is_self_pronoun(lhs, builder, self_pronoun_valid) =>
                {
                    match lhs {
                        SubjectRefAst::TriggerSubject if in_trigger => {
                            Some((MoveSubjectRef::TriggeringObject, filter))
                        }
                        SubjectRefAst::Anaphora(_)
                            if builder.target == BodyTarget::None
                                && in_trigger
                                && !self_pronoun_valid =>
                        {
                            Some((MoveSubjectRef::TriggeringObject, filter))
                        }
                        SubjectRefAst::AttachedHost(_) if self_pronoun_valid || in_trigger => {
                            Some((MoveSubjectRef::AttachedHost, filter))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((first_ref, tgt_f)) = hybrid_target_pair
                && let Ok(bt) = lower_battlefield_body_target(tgt_f, vocabulary)
                && bt.guarantees_creature()
                && builder.target == BodyTarget::None
            {
                builder.bind_target(bt)?;
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                }
                builder.instructions.push(Instruction::Fight {
                    first: Box::new(first_ref),
                    second: Box::new(MoveSubjectRef::TargetObject),
                });
                i += 1;
                continue;
            }
            if (self_pronoun_valid || in_trigger)
                && builder.target.guarantees_creature()
                && matches!(
                    (a.as_ref(), b.as_ref()),
                    (SubjectRefAst::SelfObject, SubjectRefAst::Anaphora(_))
                        | (SubjectRefAst::Anaphora(_), SubjectRefAst::SelfObject)
                )
            {
                if is_optional_fight {
                    builder.instructions.push(Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    });
                }
                builder.instructions.push(Instruction::Fight {
                    first: Box::new(MoveSubjectRef::AbilitySource),
                    second: Box::new(MoveSubjectRef::TargetObject),
                });
                i += 1;
                continue;
            }
        }
        // Fuse AnimateOrSetBasePT (+ optional KeepExistingTypes or "it's still a land") -> ResolvedPartSpec::SetPT + Type + Color + Abilities
        if let InstructionAst::AnimateOrSetBasePT {
            subject,
            power,
            toughness,
            description,
            keywords,
            duration,
        } = &ins[i]
        {
            let keep_types = i + 1 < ins.len()
                && (matches!(&ins[i + 1], InstructionAst::KeepExistingTypes(_))
                    || is_still_a_land_restriction(&ins[i + 1]));
            lower_animate_or_set_pt(
                subject,
                power,
                toughness,
                description.as_deref(),
                keywords,
                duration,
                keep_types,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
            i += if keep_types { 2 } else { 1 };
            continue;
        }
        lower_instruction(
            &ins[i],
            builder,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            bound_x,
        )?;
        i += 1;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn lower_animate_or_set_pt(
    subject: &SubjectRefAst,
    power: &str,
    toughness: &str,
    description: Option<&str>,
    keywords: &[String],
    duration: &DurationAst,
    keep_types: bool,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
    bound_x: Option<&ValueExprAst>,
) -> Result<(), &'static str> {
    if let Some(eb_raw) = description.and_then(|d| d.strip_prefix("earthbend:")) {
        let trimmed = eb_raw.trim();
        let (const_n, n_expr) = if trimmed == "ChosenX" {
            let expr = if let Some(bx) = bound_x {
                lower_value_expr(bx, vocabulary)?
            } else {
                ValueExpr::chosen_x()
            };
            (None, expr)
        } else if let Some(inner) = trimmed
            .strip_prefix("Const(")
            .and_then(|s| s.strip_suffix(')'))
        {
            let n = inner
                .trim()
                .parse::<i64>()
                .map_err(|_| "oracle:unsupported-earthbend-count")?;
            (Some(n), ValueExpr::literal(n))
        } else if let Ok(n) = trimmed.parse::<i64>() {
            (Some(n), ValueExpr::literal(n))
        } else {
            return Err("oracle:unsupported-earthbend-count");
        };
        let watch_land = ObjectFilter::land().controlled();
        builder.bind_target(BodyTarget::FilteredPermanent(watch_land))?;
        builder
            .instructions
            .push(Instruction::CreateContinuousEffect {
                parts: ResolvedPartsSpec(vec![
                    ResolvedPartSpec::Type(TypeChangeSpec {
                        land_subtypes: SubtypeOperation::Keep,
                        supertypes: TypeOperation::Keep,
                        types: TypeOperation::Add(BTreeSet::from([CardType::Creature])),
                        subtypes: SubtypeOperation::Keep,
                    }),
                    ResolvedPartSpec::Abilities(AbilityChange::grant_keywords([
                        KeywordAbility::Haste,
                    ])),
                    ResolvedPartSpec::SetPT(PowerToughness {
                        power: 0.into(),
                        toughness: 0.into(),
                    }),
                ]),
                duration: ResolvedDuration::NoStatedDuration,
            });
        if let Some(n) = const_n
            && n > 0
        {
            builder
                .instructions
                .push(Instruction::PutPowerToughnessCounters {
                    counters: PowerToughnessCounters {
                        plus_one: n.into(),
                        minus_one: 0.into(),
                    },
                });
        } else {
            builder
                .instructions
                .push(Instruction::PutNamedCounterOnTarget {
                    kind: CounterKind::PlusOne,
                    amount: n_expr,
                });
        }
        let return_body = ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::MoveSubject {
                subject: MoveSubjectRef::TriggeringObject,
                destination: SubjectDestination::Battlefield {
                    tapped: true,
                    attacking: false,
                    transformed: false,
                    under_owner_control: false,
                    with_counter: None,
                    attach_to_self: false,
                },
            }],
        );
        builder
            .instructions
            .push(Instruction::ScheduleDelayedEventTrigger {
                trigger: Box::new(TriggerSpec::new(TriggerEvent::AnyOf(vec![
                    TriggerEvent::Dies {
                        subject: watch_land,
                        mode: DiesTriggerMode::Permanent,
                    },
                    TriggerEvent::Exiled {
                        subject: watch_land,
                        from_zone: TriggerFromZone::Battlefield,
                    },
                ]))),
                body: Box::new(return_body),
                expires: ResolvedDuration::NoStatedDuration,
                once: true,
            });
        return Ok(());
    }
    if let SubjectRefAst::Compound(items) = subject
        && !items.is_empty()
    {
        for it in items {
            lower_animate_or_set_pt(
                it,
                power,
                toughness,
                description,
                keywords,
                duration,
                keep_types,
                builder,
                vocabulary,
                in_trigger,
                self_pronoun_valid,
                bound_x,
            )?;
        }
        return Ok(());
    }
    let parse_pt_expr = |raw: &str| -> Result<ValueExpr, &'static str> {
        let s = raw.trim();
        if let Ok(n) = s.parse::<i64>() {
            return Ok(ValueExpr::literal(n));
        }
        if s.eq_ignore_ascii_case("x") {
            return if let Some(bx) = bound_x {
                lower_value_expr(bx, vocabulary)
            } else {
                Ok(ValueExpr::chosen_x())
            };
        }
        if let Ok(ast_e) = spellwake_oracle_compiler::filter::parse_value_expr(
            s,
            &cached_compiler_vocab(vocabulary),
        ) {
            return lower_value_expr(resolve_x(&ast_e, bound_x), vocabulary);
        }
        Err("oracle:animate-non-const-pt")
    };
    let mut inline_pt_expr: Option<ValueExpr> = None;
    let mut inline_kws: Vec<String> = Vec::new();
    let mut effective_desc = description;
    if let Some(desc) = description {
        let desc_low = desc.to_ascii_lowercase();
        for pat in [
            " with base power and toughness each equal to ",
            " with base power and toughness equal to ",
            " with power and toughness each equal to ",
            " with power and toughness equal to ",
        ] {
            if let Some(idx) = desc_low.find(pat) {
                let ty_part = desc[..idx].trim();
                let tail = desc[idx + pat.len()..].trim().trim_end_matches('.');
                let tail_low = tail.to_ascii_lowercase();
                let (val_raw, kw_tail) = [" and gains ", " and gain ", " and has ", " and have "]
                    .iter()
                    .find_map(|sep| {
                        tail_low.find(sep).map(|k_idx| {
                            (tail[..k_idx].trim(), Some(tail[k_idx + sep.len()..].trim()))
                        })
                    })
                    .unwrap_or((tail, None));
                let val_expr = parse_pt_expr(val_raw)?;
                if let Some(kw_raw) = kw_tail {
                    for piece in kw_raw.split(',').flat_map(|seg| seg.split(" and ")) {
                        let k = piece.trim();
                        if !k.is_empty() {
                            inline_kws.push(k.to_string());
                        }
                    }
                }
                inline_pt_expr = Some(val_expr);
                effective_desc = (!ty_part.is_empty()).then_some(ty_part);
                break;
            }
        }
    }
    let p_trim = power.trim();
    let t_trim = toughness.trim();
    let omit_pt = (p_trim.is_empty() || p_trim.eq_ignore_ascii_case("keep"))
        && (t_trim.is_empty() || t_trim.eq_ignore_ascii_case("keep"));
    let pt_part: Option<ResolvedPartSpec> = if let Some(val) = inline_pt_expr {
        Some(ResolvedPartSpec::SetPTExpr {
            power: Box::new(val.clone()),
            toughness: Box::new(val),
        })
    } else if omit_pt {
        None
    } else {
        Some(match (p_trim.parse::<i64>(), t_trim.parse::<i64>()) {
            (Ok(p), Ok(t)) => ResolvedPartSpec::SetPT(PowerToughness {
                power: p.into(),
                toughness: t.into(),
            }),
            _ => ResolvedPartSpec::SetPTExpr {
                power: Box::new(parse_pt_expr(power)?),
                toughness: Box::new(parse_pt_expr(toughness)?),
            },
        })
    };
    let dur = lower_duration(duration)?;
    let mut parts = Vec::new();
    let strip_phrase_ci = |buf: &mut String, phrase: &str| -> bool {
        let mut found = false;
        while let Some(idx) = buf.to_ascii_lowercase().find(phrase) {
            buf.replace_range(idx..idx + phrase.len(), " ");
            found = true;
        }
        found
    };
    if let Some(desc) = effective_desc {
        let mut colors = BTreeSet::new();
        let mut explicit_colorless = false;
        let mut add_colors = false;
        let mut loses_all_abilities = false;
        let mut supertypes = BTreeSet::new();
        let mut card_types = BTreeSet::new();
        let mut land_subtypes = BTreeSet::new();
        let mut artifact_subtypes = BTreeSet::new();
        let mut enchantment_subtypes = BTreeSet::new();
        let mut subtypes = BTreeSet::new();
        let mut extra_kws = std::mem::take(&mut inline_kws);
        let lower_desc = desc.to_ascii_lowercase();
        let mut add_types = keep_types
            || lower_desc.contains("in addition to its other types")
            || lower_desc.contains("in addition to their other types")
            || lower_desc.contains("in addition to its other creature types")
            || lower_desc.contains("in addition to their other creature types")
            || lower_desc.contains("in addition to its other land types")
            || lower_desc.contains("in addition to their other land types")
            || lower_desc.contains("in addition to its other colors and types")
            || lower_desc.contains("in addition to their other colors and types")
            || lower_desc.contains("still a ")
            || lower_desc.contains("still an ")
            || lower_desc.contains("still lands")
            || lower_desc.contains("still artifacts")
            || lower_desc.contains("still enchantments")
            || lower_desc.contains("still creatures");
        if lower_desc.contains("in addition to its other colors")
            || lower_desc.contains("in addition to their other colors")
        {
            add_colors = true;
        }
        let mut clean = desc.to_string();
        for phrase in [
            "in addition to its other colors and types",
            "in addition to their other colors and types",
            "in addition to its other creature types",
            "in addition to their other creature types",
            "in addition to its other land types",
            "in addition to their other land types",
            "in addition to its other types",
            "in addition to their other types",
            "in addition to its other colors",
            "in addition to their other colors",
        ] {
            strip_phrase_ci(&mut clean, phrase);
        }
        let mut all_creature_types = false;
        for phrase in [
            "gains all creature types",
            "gain all creature types",
            "has all creature types",
            "have all creature types",
            "gains every creature type",
            "gain every creature type",
            "has every creature type",
            "have every creature type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_creature_types = true;
                add_types = true;
            }
        }
        for phrase in [
            "becomes all creature types",
            "become all creature types",
            "becomes every creature type",
            "become every creature type",
            "is all creature types",
            "are all creature types",
            "is every creature type",
            "are every creature type",
            "all creature types",
            "every creature type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_creature_types = true;
            }
        }
        if all_creature_types {
            for ord in 0..vocabulary.len() {
                if let Ok(ordinal) = u16::try_from(ord)
                    && let Some(ct) = vocabulary.resolve_member(vocabulary.id(), ordinal)
                {
                    subtypes.insert(ct);
                }
            }
        }
        let mut all_basic_land_types = false;
        for phrase in [
            "gains all basic land types",
            "gain all basic land types",
            "has all basic land types",
            "have all basic land types",
            "gains every basic land type",
            "gain every basic land type",
            "has every basic land type",
            "have every basic land type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_basic_land_types = true;
                add_types = true;
            }
        }
        for phrase in [
            "becomes all basic land types",
            "become all basic land types",
            "becomes every basic land type",
            "become every basic land type",
            "is all basic land types",
            "are all basic land types",
            "is every basic land type",
            "are every basic land type",
            "all basic land types",
            "every basic land type",
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                all_basic_land_types = true;
            }
        }
        if all_basic_land_types {
            land_subtypes.extend([
                LandType::Plains,
                LandType::Island,
                LandType::Swamp,
                LandType::Mountain,
                LandType::Forest,
            ]);
        }
        if strip_phrase_ci(&mut clean, "all colors") {
            colors.extend([
                Color::White,
                Color::Blue,
                Color::Black,
                Color::Red,
                Color::Green,
            ]);
        }
        if strip_phrase_ci(&mut clean, "loses all abilities")
            || strip_phrase_ci(&mut clean, "lose all abilities")
        {
            loses_all_abilities = true;
        }
        for phrase in [
            "until end of turn",
            "until your next turn",
            "it's still a land.",
            "it's still a land",
            "they're still lands.",
            "they're still lands",
            "it's still an artifact.",
            "it's still an artifact",
            "they're still artifacts.",
            "they're still artifacts",
            "it's still an enchantment.",
            "it's still an enchantment",
            "they're still enchantments.",
            "they're still enchantments",
            "it's still a creature.",
            "it's still a creature",
            "they're still creatures.",
            "they're still creatures",
        ] {
            strip_phrase_ci(&mut clean, phrase);
        }
        clean = clean.replace(" and ", " ");
        for (phrase, kw_label) in [
            ("first strike", "first strike"),
            ("double strike", "double strike"),
        ] {
            if strip_phrase_ci(&mut clean, phrase) {
                extra_kws.push(kw_label.to_string());
            }
        }
        for raw_tok in clean.split_whitespace() {
            let tok = raw_tok
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'' && c != '/');
            if tok.is_empty() {
                continue;
            }
            if tok.contains('/')
                && tok.split('/').all(|seg| {
                    seg.parse::<i64>().is_ok() || seg.eq_ignore_ascii_case("x") || seg == "*"
                })
            {
                continue;
            }
            let tok = tok.trim_matches('/');
            if tok.is_empty() {
                continue;
            }
            match tok.to_ascii_lowercase().as_str() {
                "a" | "an" | "with" | "still" | "that's" | "it's" | "they're" | "also" => {}
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
                    explicit_colorless = true;
                }
                "basic" => {
                    supertypes.insert(Supertype::Basic);
                }
                "legendary" => {
                    supertypes.insert(Supertype::Legendary);
                }
                "snow" => {
                    supertypes.insert(Supertype::Snow);
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
                "treasure" | "treasures" => {
                    artifact_subtypes.insert(ArtifactSubtype::Treasure);
                }
                "food" | "foods" => {
                    artifact_subtypes.insert(ArtifactSubtype::Food);
                }
                "clue" | "clues" => {
                    artifact_subtypes.insert(ArtifactSubtype::Clue);
                }
                "blood" => {
                    artifact_subtypes.insert(ArtifactSubtype::Blood);
                }
                "gold" => {
                    artifact_subtypes.insert(ArtifactSubtype::Gold);
                }
                "vehicle" | "vehicles" => {
                    artifact_subtypes.insert(ArtifactSubtype::Vehicle);
                }
                "equipment" => {
                    artifact_subtypes.insert(ArtifactSubtype::Equipment);
                }
                "junk" => {
                    artifact_subtypes.insert(ArtifactSubtype::Junk);
                }
                "map" | "maps" => {
                    artifact_subtypes.insert(ArtifactSubtype::Map);
                }
                "powerstone" | "powerstones" => {
                    artifact_subtypes.insert(ArtifactSubtype::Powerstone);
                }
                "cartouche" | "cartouches" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Cartouche);
                }
                "shard" | "shards" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Shard);
                }
                "saga" | "sagas" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Saga);
                }
                "shrine" | "shrines" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Shrine);
                }
                "curse" | "curses" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Curse);
                }
                "role" | "roles" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Role);
                }
                "room" | "rooms" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Room);
                }
                "class" | "classes" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Class);
                }
                "case" | "cases" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Case);
                }
                "rune" | "runes" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Rune);
                }
                "background" | "backgrounds" => {
                    enchantment_subtypes.insert(EnchantmentSubtype::Background);
                }
                kw_cand if lower_keyword_ability_name(kw_cand).is_some() => {
                    extra_kws.push(kw_cand.to_string());
                }
                _ => {
                    let resolve_ci = |w: &str| -> Option<CreatureTypeId> {
                        if let Some(ct) = vocabulary.resolve_label(w) {
                            return Some(ct);
                        }
                        let title = w
                            .split('-')
                            .map(|seg| {
                                let mut ch = seg.chars();
                                ch.next()
                                    .map(|c| c.to_uppercase().collect::<String>() + ch.as_str())
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<_>>()
                            .join("-");
                        vocabulary.resolve_label(&title)
                    };
                    if let Some(ct) = resolve_ci(tok) {
                        subtypes.insert(ct);
                    } else if let Some(singular) = tok.strip_suffix("ves")
                        && let Some(ct) = resolve_ci(&format!("{singular}f"))
                    {
                        subtypes.insert(ct);
                    } else if let Some(singular) = tok.strip_suffix("es")
                        && let Some(ct) = resolve_ci(singular)
                    {
                        subtypes.insert(ct);
                    } else if let Some(singular) = tok.strip_suffix('s')
                        && let Some(ct) = resolve_ci(singular)
                    {
                        subtypes.insert(ct);
                    } else {
                        return Err("oracle:unsupported-animate-desc");
                    }
                }
            }
        }
        if !supertypes.is_empty()
            || !card_types.is_empty()
            || !subtypes.is_empty()
            || !land_subtypes.is_empty()
            || !artifact_subtypes.is_empty()
            || !enchantment_subtypes.is_empty()
        {
            let super_op = if supertypes.is_empty() {
                TypeOperation::Keep
            } else if add_types {
                TypeOperation::Add(supertypes)
            } else {
                TypeOperation::Set(supertypes)
            };
            let types_op = if card_types.is_empty() {
                TypeOperation::Keep
            } else if add_types {
                TypeOperation::Add(card_types)
            } else {
                TypeOperation::Set(card_types)
            };
            let sub_op = if subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if add_types {
                SubtypeOperation::Add(InstructionSubtypeOperand::Literal(subtypes))
            } else {
                SubtypeOperation::Set(InstructionSubtypeOperand::Literal(subtypes))
            };
            let land_op = if land_subtypes.is_empty() {
                SubtypeOperation::Keep
            } else if add_types {
                SubtypeOperation::Add(land_subtypes)
            } else {
                SubtypeOperation::Set(land_subtypes)
            };
            let base = TypeChangeSpec {
                land_subtypes: land_op,
                supertypes: super_op,
                types: types_op,
                subtypes: sub_op,
            };
            if artifact_subtypes.is_empty() && enchantment_subtypes.is_empty() {
                parts.push(ResolvedPartSpec::Type(base));
            } else {
                let art_op = if artifact_subtypes.is_empty() {
                    SubtypeOperation::Keep
                } else if add_types {
                    SubtypeOperation::Add(artifact_subtypes)
                } else {
                    SubtypeOperation::Set(artifact_subtypes)
                };
                let ench_op = if enchantment_subtypes.is_empty() {
                    SubtypeOperation::Keep
                } else if add_types {
                    SubtypeOperation::Add(enchantment_subtypes)
                } else {
                    SubtypeOperation::Set(enchantment_subtypes)
                };
                parts.push(ResolvedPartSpec::TypeWithSubtypes {
                    base,
                    artifact_subtypes: art_op,
                    enchantment_subtypes: ench_op,
                });
            }
        }
        if !colors.is_empty() {
            parts.push(ResolvedPartSpec::Color(if add_colors {
                ColorOperation::Add(colors)
            } else {
                ColorOperation::Set(colors)
            }));
        } else if explicit_colorless {
            parts.push(ResolvedPartSpec::Color(
                ColorOperation::Set(BTreeSet::new()),
            ));
        }
        if loses_all_abilities || !extra_kws.is_empty() || !keywords.is_empty() {
            let mut merged: Vec<String> = keywords.to_vec();
            merged.extend(extra_kws);
            let mut ab_change = if !merged.is_empty() {
                lower_ability_change(&merged, vocabulary)?
            } else {
                AbilityChange::default()
            };
            if loses_all_abilities {
                ab_change.removal = Some(AbilityRemoval::All);
            }
            parts.push(ResolvedPartSpec::Abilities(ab_change));
        }
    } else if !keywords.is_empty() || !inline_kws.is_empty() {
        let mut merged: Vec<String> = keywords.to_vec();
        merged.extend(inline_kws);
        parts.push(ResolvedPartSpec::Abilities(lower_ability_change(
            &merged, vocabulary,
        )?));
    }
    if let Some(pt) = pt_part {
        parts.push(pt);
    }
    if parts.is_empty() {
        return Err("oracle:unsupported-animate-desc");
    }
    let spec = ResolvedPartsSpec(parts);
    let has_perm_target = matches!(
        builder.target,
        BodyTarget::SingleCreature
            | BodyTarget::SinglePermanent
            | BodyTarget::SinglePlaneswalker
            | BodyTarget::FilteredPermanent(_)
    );
    let has_created_tokens = builder.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::CreateTokens { .. }
                | Instruction::CreateAttackingTokens { .. }
                | Instruction::CreateTokenCopy { .. }
                | Instruction::CreateTokenAndAttach { .. }
        )
    });
    let has_last_moved = builder.instructions.iter().any(|op| {
        matches!(
            op,
            Instruction::MoveSubject {
                destination: SubjectDestination::Battlefield { .. },
                ..
            }
        )
    });
    match subject {
        SubjectRefAst::SelfObject => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(a)
            if has_created_tokens
                && (a.to_ascii_lowercase().contains("token") || !has_perm_target) =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::CreatedTokens),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                ))),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(lower_object_filter(f, vocabulary)?)),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(1),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *n >= 2 => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts: spec,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) && *max > 0 => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts: spec,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let obj = lower_object_filter(&inner, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::ChosenX,
        } if matches!(filter.zone, None | Some(ZoneFilter::Battlefield)) => {
            let mut inner = filter.as_ref().clone();
            inner.zone = None;
            let bt = lower_battlefield_body_target(&inner, vocabulary)?;
            builder.bind_target(bt)?;
            builder.x_target_count = true;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: u16::MAX,
                    action: PermanentTargetAction::Modify {
                        parts: spec,
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if has_perm_target => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: spec,
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None && in_trigger && !self_pronoun_valid =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None && builder.last_group_filter.is_some() =>
        {
            let obj = builder
                .last_group_filter
                .ok_or("oracle:unsupported-animate-subject")?;
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(obj)),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if builder.target == BodyTarget::None && has_last_moved => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::LastMoved),
                parts: spec,
                duration: dur,
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-animate-subject"),
    }
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
