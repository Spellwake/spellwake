use super::*;

pub(in super::super) type ExtractedModal<'a> = (
    &'a [InstructionAst],
    &'a str,
    &'a [spellwake_oracle_compiler::ast::ModalModeAst],
    &'a [InstructionAst],
    bool,
);

pub(in super::super) fn extract_modal_with_wrap(
    ins: &[InstructionAst],
) -> Option<ExtractedModal<'_>> {
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
    if modes.iter().any(|m| m.granted.is_some()) {
        return None;
    }
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

pub(super) fn strip_each_mode_different_player(
    ops: &[InstructionAst],
) -> (Vec<InstructionAst>, bool) {
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

pub(super) fn subject_has_upto_one_target(subject: &SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::TargetObject {
            count: TargetCountAst::UpTo(1),
            ..
        }
        | SubjectRefAst::TargetPlayer {
            count: TargetCountAst::UpTo(1),
            ..
        }
        | SubjectRefAst::TargetOneOf {
            count: TargetCountAst::UpTo(1),
            ..
        } => true,
        SubjectRefAst::TargetOneOf { options, .. } => {
            options.iter().any(subject_has_upto_one_target)
        }
        SubjectRefAst::Compound(items) => items.iter().any(subject_has_upto_one_target),
        SubjectRefAst::InZone { subject, .. }
        | SubjectRefAst::ControllerOf(subject)
        | SubjectRefAst::OwnerOf(subject) => subject_has_upto_one_target(subject),
        _ => false,
    }
}

pub(super) fn instruction_has_upto_one_target(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::DealDamage { source, target, .. } => {
            subject_has_upto_one_target(source) || subject_has_upto_one_target(target)
        }
        InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. }
        | InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::GrantKeywordChoice { subject, .. }
        | InstructionAst::RemoveKeywords { subject, .. }
        | InstructionAst::RemoveAllAbilities { subject, .. }
        | InstructionAst::PutCounters { subject, .. }
        | InstructionAst::RemoveCounters { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::ExileUntilLeaves { subject }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::ReturnHalf { subject, .. }
        | InstructionAst::MoveZone { subject, .. }
        | InstructionAst::MoveCounted { subject, .. }
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
        | InstructionAst::TapOrUntapPermanent { subject }
        | InstructionAst::SkipNextUntap { subject }
        | InstructionAst::Regenerate { subject }
        | InstructionAst::Transform { subject }
        | InstructionAst::PhaseOut { subject }
        | InstructionAst::PhaseIn { subject }
        | InstructionAst::PhaseOutUntil { subject, .. }
        | InstructionAst::TurnFaceDown { subject }
        | InstructionAst::TurnFaceUp { subject, .. }
        | InstructionAst::GainControl { subject, .. }
        | InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. }
        | InstructionAst::DoubleStatOrCounters { subject, .. }
        | InstructionAst::BecomeChosenCharacteristic { subject, .. }
        | InstructionAst::SetBasicLandType { subject, .. }
        | InstructionAst::RemoveSupertype { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::ChangeBlockerLimit { subject, .. }
        | InstructionAst::SwitchPowerToughness { subject, .. }
        | InstructionAst::Explore { subject }
        | InstructionAst::Detain { subject }
        | InstructionAst::Suspect { subject }
        | InstructionAst::SetSuspected { subject, .. }
        | InstructionAst::SetGoaded { subject, .. }
        | InstructionAst::Exert { subject }
        | InstructionAst::Harness { subject }
        | InstructionAst::UnlockDoor { subject }
        | InstructionAst::BecomePrepared { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::GrantCastPermission { subject, .. }
        | InstructionAst::Sacrifice { subject, .. }
        | InstructionAst::SacrificeUnlessPay { subject, .. }
        | InstructionAst::Copy { subject, .. } => subject_has_upto_one_target(subject),
        InstructionAst::Airbend { subject } => {
            subject.as_deref().is_some_and(subject_has_upto_one_target)
        }
        InstructionAst::CreateToken {
            token: TokenSpecAst::Role { attached_to, .. },
            ..
        }
        | InstructionAst::CreateTokenWithAbilities {
            token: TokenSpecAst::Role { attached_to, .. },
            ..
        } => subject_has_upto_one_target(attached_to),
        InstructionAst::CreateToken {
            token: TokenSpecAst::Copy { source, .. },
            ..
        }
        | InstructionAst::CreateTokenWithAbilities {
            token: TokenSpecAst::Copy { source, .. },
            ..
        } => subject_has_upto_one_target(source),
        InstructionAst::BecomeCopy {
            subject, source, ..
        } => subject_has_upto_one_target(subject) || subject_has_upto_one_target(source),
        InstructionAst::ForcedBlockByFilter {
            blockers, target, ..
        } => subject_has_upto_one_target(blockers) || subject_has_upto_one_target(target),
        InstructionAst::Counter { target, .. } | InstructionAst::PreventDamage { target, .. } => {
            subject_has_upto_one_target(target)
        }
        InstructionAst::PreventAllDamage { target, .. } => {
            target.as_deref().is_some_and(subject_has_upto_one_target)
        }
        InstructionAst::PreventAllDamageFrom {
            source, recipient, ..
        } => {
            subject_has_upto_one_target(source)
                || recipient
                    .as_deref()
                    .is_some_and(subject_has_upto_one_target)
        }
        InstructionAst::Attach {
            subject, target, ..
        } => subject_has_upto_one_target(subject) || subject_has_upto_one_target(target),
        InstructionAst::MoveCounters { from, to, .. }
        | InstructionAst::MoveCounter { from, to, .. } => {
            subject_has_upto_one_target(from) || subject_has_upto_one_target(to)
        }
        InstructionAst::Fight { a, b } => {
            subject_has_upto_one_target(a) || subject_has_upto_one_target(b)
        }
        InstructionAst::Exchange { left, right, .. } => {
            subject_has_upto_one_target(left)
                || right.as_deref().is_some_and(subject_has_upto_one_target)
        }
        InstructionAst::Optional(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Simultaneous(inner)
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::ScopedDuration { body: inner, .. } => {
            inner.iter().any(instruction_has_upto_one_target)
        }
        _ => false,
    }
}

pub(super) fn rewrite_subject_upto_one_to_exact_one(subject: &mut SubjectRefAst) -> bool {
    match subject {
        SubjectRefAst::TargetObject { count, .. }
        | SubjectRefAst::TargetPlayer { count, .. }
        | SubjectRefAst::TargetOneOf { count, .. }
            if *count == TargetCountAst::UpTo(1) =>
        {
            *count = TargetCountAst::Exact(1);
            true
        }
        SubjectRefAst::TargetOneOf { options, .. } => {
            let mut changed = false;
            for item in options {
                changed |= rewrite_subject_upto_one_to_exact_one(item);
            }
            changed
        }
        SubjectRefAst::Compound(items) => {
            let mut changed = false;
            for item in items {
                changed |= rewrite_subject_upto_one_to_exact_one(item);
            }
            changed
        }
        SubjectRefAst::InZone { subject, .. }
        | SubjectRefAst::ControllerOf(subject)
        | SubjectRefAst::OwnerOf(subject) => rewrite_subject_upto_one_to_exact_one(subject),
        _ => false,
    }
}

pub(super) fn rewrite_instruction_upto_one_to_exact_one(ins: &mut InstructionAst) -> bool {
    match ins {
        InstructionAst::DealDamage { source, target, .. } => {
            let a = rewrite_subject_upto_one_to_exact_one(source);
            let b = rewrite_subject_upto_one_to_exact_one(target);
            a || b
        }
        InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. }
        | InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::GrantKeywordChoice { subject, .. }
        | InstructionAst::RemoveKeywords { subject, .. }
        | InstructionAst::RemoveAllAbilities { subject, .. }
        | InstructionAst::PutCounters { subject, .. }
        | InstructionAst::RemoveCounters { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::ExileUntilLeaves { subject }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::ReturnHalf { subject, .. }
        | InstructionAst::MoveZone { subject, .. }
        | InstructionAst::MoveCounted { subject, .. }
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
        | InstructionAst::TapOrUntapPermanent { subject }
        | InstructionAst::SkipNextUntap { subject }
        | InstructionAst::Regenerate { subject }
        | InstructionAst::Transform { subject }
        | InstructionAst::PhaseOut { subject }
        | InstructionAst::PhaseIn { subject }
        | InstructionAst::PhaseOutUntil { subject, .. }
        | InstructionAst::TurnFaceDown { subject }
        | InstructionAst::TurnFaceUp { subject, .. }
        | InstructionAst::GainControl { subject, .. }
        | InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. }
        | InstructionAst::DoubleStatOrCounters { subject, .. }
        | InstructionAst::BecomeChosenCharacteristic { subject, .. }
        | InstructionAst::SetBasicLandType { subject, .. }
        | InstructionAst::RemoveSupertype { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::ChangeBlockerLimit { subject, .. }
        | InstructionAst::SwitchPowerToughness { subject, .. }
        | InstructionAst::Explore { subject }
        | InstructionAst::Detain { subject }
        | InstructionAst::Suspect { subject }
        | InstructionAst::SetSuspected { subject, .. }
        | InstructionAst::SetGoaded { subject, .. }
        | InstructionAst::Exert { subject }
        | InstructionAst::Harness { subject }
        | InstructionAst::UnlockDoor { subject }
        | InstructionAst::BecomePrepared { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::GrantCastPermission { subject, .. }
        | InstructionAst::Sacrifice { subject, .. }
        | InstructionAst::SacrificeUnlessPay { subject, .. }
        | InstructionAst::Copy { subject, .. } => rewrite_subject_upto_one_to_exact_one(subject),
        InstructionAst::Airbend { subject } => subject
            .as_deref_mut()
            .is_some_and(rewrite_subject_upto_one_to_exact_one),
        InstructionAst::CreateToken {
            token: TokenSpecAst::Role { attached_to, .. },
            ..
        }
        | InstructionAst::CreateTokenWithAbilities {
            token: TokenSpecAst::Role { attached_to, .. },
            ..
        } => rewrite_subject_upto_one_to_exact_one(attached_to),
        InstructionAst::CreateToken {
            token: TokenSpecAst::Copy { source, .. },
            ..
        }
        | InstructionAst::CreateTokenWithAbilities {
            token: TokenSpecAst::Copy { source, .. },
            ..
        } => rewrite_subject_upto_one_to_exact_one(source),
        InstructionAst::BecomeCopy {
            subject, source, ..
        } => {
            let a = rewrite_subject_upto_one_to_exact_one(subject);
            let b = rewrite_subject_upto_one_to_exact_one(source);
            a || b
        }
        InstructionAst::ForcedBlockByFilter {
            blockers, target, ..
        } => {
            let a = rewrite_subject_upto_one_to_exact_one(blockers);
            let b = rewrite_subject_upto_one_to_exact_one(target);
            a || b
        }
        InstructionAst::Counter { target, .. } | InstructionAst::PreventDamage { target, .. } => {
            rewrite_subject_upto_one_to_exact_one(target)
        }
        InstructionAst::PreventAllDamage { target, .. } => target
            .as_deref_mut()
            .is_some_and(rewrite_subject_upto_one_to_exact_one),
        InstructionAst::PreventAllDamageFrom {
            source, recipient, ..
        } => {
            let a = rewrite_subject_upto_one_to_exact_one(source);
            let b = recipient
                .as_deref_mut()
                .is_some_and(rewrite_subject_upto_one_to_exact_one);
            a || b
        }
        InstructionAst::Attach {
            subject, target, ..
        } => {
            let a = rewrite_subject_upto_one_to_exact_one(subject);
            let b = rewrite_subject_upto_one_to_exact_one(target);
            a || b
        }
        InstructionAst::MoveCounters { from, to, .. }
        | InstructionAst::MoveCounter { from, to, .. } => {
            let a = rewrite_subject_upto_one_to_exact_one(from);
            let b = rewrite_subject_upto_one_to_exact_one(to);
            a || b
        }
        InstructionAst::Fight { a, b } => {
            let x = rewrite_subject_upto_one_to_exact_one(a);
            let y = rewrite_subject_upto_one_to_exact_one(b);
            x || y
        }
        InstructionAst::Exchange { left, right, .. } => {
            let a = rewrite_subject_upto_one_to_exact_one(left);
            let b = right
                .as_deref_mut()
                .is_some_and(rewrite_subject_upto_one_to_exact_one);
            a || b
        }
        InstructionAst::Optional(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Simultaneous(inner)
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::ScopedDuration { body: inner, .. } => {
            let mut changed = false;
            for item in inner {
                changed |= rewrite_instruction_upto_one_to_exact_one(item);
            }
            changed
        }
        _ => false,
    }
}

fn body_has_dependent_targets(body: &ExecutableBody) -> bool {
    matches!(body.target, BodyTarget::OpponentOrPlaneswalkerThenCreature)
        || (body.target == BodyTarget::SinglePlayer
            && matches!(
                body.instructions.first(),
                Some(Instruction::ShuffleTargetGraveyardCards { .. })
            ))
}

fn body_safe_for_synthesized_mode(body: &ExecutableBody) -> bool {
    !body.instructions.is_empty()
        && body.triggered_modes().is_none()
        && body.ordered_any_target_bounds().is_none()
        && !body_has_dependent_targets(body)
        && body.target_division_total().is_none()
        && body.target_division_expr().is_none()
        && !body.has_x_target_count()
        && !body.has_up_to_x_target_count()
        && !body.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::BindOrderedPermanents { .. }
                    | Instruction::ShuffleOnePermanentPerPlayer
                    | Instruction::ReturnOneNonlandPerOpponent
                    | Instruction::ExileOnePerPlayerUntilSourceLeaves
            )
        })
}

fn primary_modifier_subject(ins: &InstructionAst) -> Option<(&SubjectRefAst, u8)> {
    match ins {
        InstructionAst::RemoveAllAbilities { subject, .. } => Some((subject.as_ref(), 1)),
        InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. } => Some((subject.as_ref(), 2)),
        InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. } => Some((subject.as_ref(), 3)),
        InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::GrantKeywordChoice { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::BecomeChosenCharacteristic { subject, .. } => Some((subject.as_ref(), 4)),
        _ => None,
    }
}

fn is_single_clause_repeated_modifier_pair(a: &[InstructionAst], b: &[InstructionAst]) -> bool {
    let ([op_a], [op_b]) = (a, b) else {
        return false;
    };
    let (Some((subj_a, kind_a)), Some((subj_b, kind_b))) = (
        primary_modifier_subject(op_a),
        primary_modifier_subject(op_b),
    ) else {
        return false;
    };
    subj_a == subj_b
        && matches!(
            (kind_a, kind_b),
            (1, 2) | (2, 1) | (3, 4) | (4, 3) | (1, 4) | (4, 1) | (2, 4) | (4, 2)
        )
}

enum OuterWrap<'a> {
    None,
    ScopedDuration(&'a DurationAst),
    BindX(&'a ValueExprAst),
}

fn try_lower_exact_plus_upto_modes_core(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    trigger_hint: TriggerHint,
) -> Option<(u8, Vec<ExecutableBody>)> {
    let (wrap, inner_ins) = match ins {
        [InstructionAst::ScopedDuration { duration, body }] if body.len() >= 2 => {
            (OuterWrap::ScopedDuration(duration), body.as_slice())
        }
        [InstructionAst::BindX { x_expr, body }] if body.len() >= 2 => {
            (OuterWrap::BindX(x_expr.as_ref()), body.as_slice())
        }
        _ => (OuterWrap::None, ins),
    };
    let expanded = expand_top_level_compound_targets(inner_ins);
    let mut groups: Vec<(usize, usize, bool)> = Vec::new();
    let mut i = 0usize;
    while i < expanded.len() {
        let step = logical_unit_len(&expanded, i);
        let unit = &expanded[i..i + step];
        let has_upto = unit.iter().any(instruction_has_upto_one_target);
        let is_anaphora = !has_upto && unit.iter().any(instruction_references_target_anaphora);
        if is_anaphora && let Some(last) = groups.last_mut() {
            last.1 = i + step;
        } else {
            groups.push((i, i + step, has_upto));
        }
        i += step;
    }
    let upto_groups: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter_map(|(idx, &(_, _, has_upto))| has_upto.then_some(idx))
        .collect();
    if !(1..=3).contains(&upto_groups.len()) || groups.len() < 2 {
        return None;
    }
    // Fail closed if adjacent upto groups are a single-sentence repeated-target conjunction
    // (such as "loses all abilities and becomes a 4/4" or "gets +1/+0 and gains flying").
    for pair in upto_groups.windows(2) {
        let (a_start, a_end, _) = groups[pair[0]];
        let (b_start, b_end, _) = groups[pair[1]];
        if is_single_clause_repeated_modifier_pair(
            &expanded[a_start..a_end],
            &expanded[b_start..b_end],
        ) {
            return None;
        }
    }
    let k = upto_groups.len();
    let subset_count = 1usize << k;
    let mut options = Vec::with_capacity(subset_count);
    let mut has_empty_subset = false;
    for mask in 0..subset_count {
        let mut subset_ins = Vec::with_capacity(expanded.len());
        let mut upto_pos = 0usize;
        for &(g_start, g_end, has_upto) in &groups {
            if has_upto {
                let include = (mask >> upto_pos) & 1 != 0;
                upto_pos += 1;
                if include {
                    let start_len = subset_ins.len();
                    subset_ins.extend_from_slice(&expanded[g_start..g_end]);
                    let mut rewritten = false;
                    for item in &mut subset_ins[start_len..] {
                        rewritten |= rewrite_instruction_upto_one_to_exact_one(item);
                    }
                    if !rewritten {
                        return None;
                    }
                }
            } else {
                subset_ins.extend_from_slice(&expanded[g_start..g_end]);
            }
        }
        if subset_ins.is_empty() {
            has_empty_subset = true;
            continue;
        }
        // Skip empty-only BindX wrappers when all real action clauses were excluded.
        let all_empty_bind_x = subset_ins
            .iter()
            .all(|op| matches!(op, InstructionAst::BindX { body, .. } if body.is_empty()));
        if all_empty_bind_x {
            has_empty_subset = true;
            continue;
        }
        let wrapped_ins = match wrap {
            OuterWrap::None => subset_ins,
            OuterWrap::ScopedDuration(dur) => vec![InstructionAst::ScopedDuration {
                duration: dur.clone(),
                body: subset_ins,
            }],
            OuterWrap::BindX(x_expr) => vec![InstructionAst::BindX {
                x_expr: Box::new(x_expr.clone()),
                body: subset_ins,
            }],
        };
        let mut lowered = lower_effect_body_with_hint(
            &wrapped_ins,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            true,
            trigger_hint,
        )
        .ok()?;
        if !body_safe_for_synthesized_mode(&lowered) {
            return None;
        }
        if !in_trigger {
            let saw_sacrifice = lowered.instructions.iter().any(|op| {
                matches!(op, Instruction::Sacrifice { .. })
                    || crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                        .is_some_and(|spec| {
                            matches!(
                                spec.destination,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                                }
                            )
                        })
            });
            if saw_sacrifice {
                rewrite_body_anaphora_stats(&mut lowered, ValueObjectRef::LastSacrificedCreature);
            }
        }
        options.push(lowered);
    }
    if options.len() < 2 {
        return None;
    }
    let minimum = if has_empty_subset { 0 } else { 1 };
    Some((minimum, options))
}

pub(super) fn try_lower_exact_plus_upto_spell_modes(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<(ExecutableBody, SpellModes)> {
    let (minimum, mut options) = try_lower_exact_plus_upto_modes_core(
        ins,
        vocabulary,
        ids,
        false,
        false,
        TriggerHint::None,
    )?;
    let first = options.remove(0);
    Some((
        first,
        SpellModes {
            minimum,
            maximum: 1,
            conditional_maximum: None,
            allow_repeat: false,
            distinct_targets: false,
            uniqueness: ModalUniqueness::None,
            mode_costs: Vec::new(),
            escalate_mana: None,
            prefix: None,
            suffix: None,
            additional: options,
        },
    ))
}

pub(in super::super) fn try_lower_exact_plus_upto_activated_modes(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<(ExecutableBody, SpellModes)> {
    let (minimum, mut options) =
        try_lower_exact_plus_upto_modes_core(ins, vocabulary, ids, false, true, TriggerHint::None)?;
    let first = options.remove(0);
    Some((
        first,
        SpellModes {
            minimum,
            maximum: 1,
            conditional_maximum: None,
            allow_repeat: false,
            distinct_targets: false,
            uniqueness: ModalUniqueness::None,
            mode_costs: Vec::new(),
            escalate_mana: None,
            prefix: None,
            suffix: None,
            additional: options,
        },
    ))
}

pub(in super::super) fn try_lower_exact_plus_upto_triggered_modes(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    spec: &TriggerSpec,
    self_pronoun_valid: bool,
    guard: Option<InterveningCondition>,
) -> Option<ExecutableBody> {
    let hint = TriggerHint::from_event(&spec.event);
    let (minimum, mut options) =
        try_lower_exact_plus_upto_modes_core(ins, vocabulary, ids, true, self_pronoun_valid, hint)?;
    if let Some(cond) = guard {
        for opt in &mut options {
            prepend_instruction_at(
                &mut opt.instructions,
                0,
                Instruction::RequireCondition(cond),
            );
        }
    }
    Some(ExecutableBody::with_triggered_modes(TriggeredModes {
        minimum,
        maximum: 1,
        distinct_targets: false,
        allow_repeat: false,
        uniqueness: ModalUniqueness::None,
        prefix: None,
        suffix: None,
        options,
    }))
}

fn try_lower_independent_multi_clause_modes_core(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    in_trigger: bool,
    self_pronoun_valid: bool,
    trigger_hint: TriggerHint,
) -> Option<Vec<ExecutableBody>> {
    let expanded = expand_top_level_compound_targets(ins);
    if expanded
        .iter()
        .any(|op| matches!(op, InstructionAst::BindX { body, .. } if body.is_empty()))
    {
        return None;
    }
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < expanded.len() {
        let step = logical_unit_len(&expanded, i);
        let unit = &expanded[i..i + step];
        let is_followup = unit.iter().any(instruction_references_target_anaphora)
            || matches!(
                unit.first(),
                Some(
                    InstructionAst::IfPreviousSucceeded(_)
                        | InstructionAst::IfPreviousFailed(_)
                        | InstructionAst::Otherwise(_)
                        | InstructionAst::ReflexiveTrigger(_)
                )
            );
        if is_followup && let Some(last) = groups.last_mut() {
            last.1 = i + step;
        } else {
            groups.push((i, i + step));
        }
        i += step;
    }
    if !(2..=6).contains(&groups.len()) {
        return None;
    }
    let mut lowered_groups = Vec::with_capacity(groups.len());
    let mut targeted_count = 0usize;
    for (idx, &(g_start, g_end)) in groups.iter().enumerate() {
        let slice = &expanded[g_start..g_end];
        if idx > 0
            && slice
                .iter()
                .any(instruction_has_that_player_controller_target)
        {
            return None;
        }
        let mut body = lower_effect_body_with_hint(
            slice,
            vocabulary,
            ids,
            in_trigger,
            self_pronoun_valid,
            false,
            trigger_hint,
        )
        .ok()?;
        if !body_safe_for_synthesized_mode(&body)
            || body
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FixedTargetClause(_)))
        {
            return None;
        }
        if !in_trigger {
            let saw_sacrifice = body.instructions.iter().any(|op| {
                matches!(op, Instruction::Sacrifice { .. })
                    || crate::keyword_creatures::oracle_lowering::toolbox_emit::unwrap_zone_op(op)
                        .is_some_and(|spec| {
                            matches!(
                                spec.destination,
                                spellwake_engine::toolbox::ZoneDest::Graveyard {
                                    semantics:
                                        spellwake_engine::toolbox::GraveyardSemantics::Sacrifice
                                }
                            )
                        })
            });
            if saw_sacrifice {
                rewrite_body_anaphora_stats(&mut body, ValueObjectRef::LastSacrificedCreature);
            }
        }
        if body.target != BodyTarget::None {
            targeted_count += 1;
        }
        lowered_groups.push(body);
    }
    if targeted_count < 2 {
        return None;
    }
    // Fail closed if any two clauses share the same BodyTarget while either carries
    // multi-target bounds: this prevents splitting a single-sentence conjunction such as
    // "Up to two target creatures each get +1/+0 and can't be blocked" into two target pools.
    for (a_idx, a) in lowered_groups.iter().enumerate() {
        if a.target == BodyTarget::None {
            continue;
        }
        for b in &lowered_groups[a_idx + 1..] {
            if a.target == b.target
                && (a.permanent_target_bounds().is_some() || b.permanent_target_bounds().is_some())
            {
                return None;
            }
        }
    }
    Some(lowered_groups)
}

pub(super) fn try_lower_independent_multi_clause_spell_modes(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<(ExecutableBody, SpellModes)> {
    let mut options = try_lower_independent_multi_clause_modes_core(
        ins,
        vocabulary,
        ids,
        false,
        false,
        TriggerHint::None,
    )?;
    let count = u8::try_from(options.len()).ok()?;
    let first = options.remove(0);
    Some((
        first,
        SpellModes {
            minimum: count,
            maximum: count,
            conditional_maximum: None,
            allow_repeat: false,
            distinct_targets: false,
            uniqueness: ModalUniqueness::None,
            mode_costs: Vec::new(),
            escalate_mana: None,
            prefix: None,
            suffix: None,
            additional: options,
        },
    ))
}

pub(in super::super) fn try_lower_independent_multi_clause_activated_modes(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Option<(ExecutableBody, SpellModes)> {
    let mut options = try_lower_independent_multi_clause_modes_core(
        ins,
        vocabulary,
        ids,
        false,
        true,
        TriggerHint::None,
    )?;
    let count = u8::try_from(options.len()).ok()?;
    let first = options.remove(0);
    Some((
        first,
        SpellModes {
            minimum: count,
            maximum: count,
            conditional_maximum: None,
            allow_repeat: false,
            distinct_targets: false,
            uniqueness: ModalUniqueness::None,
            mode_costs: Vec::new(),
            escalate_mana: None,
            prefix: None,
            suffix: None,
            additional: options,
        },
    ))
}

pub(in super::super) fn try_lower_independent_multi_clause_triggered_modes(
    ins: &[InstructionAst],
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    spec: &TriggerSpec,
    self_pronoun_valid: bool,
    guard: Option<InterveningCondition>,
) -> Option<ExecutableBody> {
    let hint = TriggerHint::from_event(&spec.event);
    let options = try_lower_independent_multi_clause_modes_core(
        ins,
        vocabulary,
        ids,
        true,
        self_pronoun_valid,
        hint,
    )?;
    let count = u8::try_from(options.len()).ok()?;
    let prefix = guard.map(|cond| {
        Box::new(ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::RequireCondition(cond)],
        ))
    });
    Some(ExecutableBody::with_triggered_modes(TriggeredModes {
        minimum: count,
        maximum: count,
        distinct_targets: false,
        allow_repeat: false,
        uniqueness: ModalUniqueness::None,
        prefix,
        suffix: None,
        options,
    }))
}

pub(super) fn parse_conditional_modal_max_phrase(raw: &str, modes_len: u8) -> Option<u8> {
    let mut norm = raw
        .trim()
        .trim_end_matches('.')
        .trim_end_matches('\u{2014}')
        .trim_end_matches('-')
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase();
    if matches!(
        norm.as_str(),
        "choose_both_instead" | "choose_both" | "choose both instead" | "choose both"
    ) {
        return (modes_len >= 2).then_some(2);
    }
    if let Some(rest) = norm.strip_prefix("instead, ") {
        norm = rest.trim().to_string();
    } else if let Some(rest) = norm.strip_prefix("instead ") {
        norm = rest.trim().to_string();
    }
    if let Some(rest) = norm.strip_suffix(" instead") {
        norm = rest.trim().to_string();
    }
    if let Some(rest) = norm.strip_prefix("you may ") {
        norm = rest.trim().to_string();
    }
    if let Some(rest) = norm.strip_prefix("choose ") {
        norm = rest.trim().to_string();
    }
    match norm.as_str() {
        "both" | "one or both" if modes_len >= 2 => Some(2),
        "all" | "all of them" | "any number" | "any_number" | "any number of them"
        | "one or more" => Some(modes_len),
        "two" | "up to two" | "one or two" | "2" | "up to 2" => Some(2),
        "three" | "up to three" | "one, two, or three" | "3" | "up to 3" => Some(3),
        "four" | "up to four" | "4" | "up to 4" => Some(4),
        "five" | "up to five" | "5" | "up to 5" => Some(5),
        _ => None,
    }
}

pub(super) fn extract_conditional_modal_max_from_body(
    body: &[InstructionAst],
    modes_len: u8,
) -> Option<u8> {
    match body {
        [InstructionAst::ReplaceEffect(inner)] | [InstructionAst::Optional(inner)] => {
            extract_conditional_modal_max_from_body(inner, modes_len)
        }
        [InstructionAst::ReplaceEffectWith(rew)] if !rew.only && rew.instead_of.is_none() => {
            extract_conditional_modal_max_from_body(&rew.body, modes_len)
        }
        [InstructionAst::ChooseOption(raw)] | [InstructionAst::ModalModifier(raw)] => {
            parse_conditional_modal_max_phrase(raw, modes_len)
        }
        [InstructionAst::Modal { choice, modes }] if modes.is_empty() => {
            parse_conditional_modal_max_phrase(choice, modes_len)
        }
        _ => None,
    }
}

pub(super) fn lower_world_modal_condition(
    cond: &ConditionAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ResolutionCondition> {
    let mut dummy = BodyBuilder::new();
    let rc = lower_resolution_condition(cond, &mut dummy, vocabulary, false, None).ok()?;
    (dummy.target == BodyTarget::None && rc.is_world_evaluable()).then_some(rc)
}

pub(super) type ConditionalModalMax = (ResolutionCondition, u8);

pub(super) fn strip_conditional_modal_max(
    ops: &[InstructionAst],
    modes_len: u8,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<(Vec<InstructionAst>, Option<ConditionalModalMax>), &'static str> {
    let mut out = Vec::with_capacity(ops.len());
    let mut found: Option<(ResolutionCondition, u8)> = None;
    for op in ops {
        if let InstructionAst::IfCondition {
            condition,
            then_body,
        } = op
            && let Some(cond_max) = extract_conditional_modal_max_from_body(then_body, modes_len)
        {
            if found.is_some() {
                return Err("oracle:multiple-conditional-modal-max");
            }
            let rc = lower_world_modal_condition(condition, vocabulary)
                .ok_or("oracle:unsupported-conditional-modal-max")?;
            found = Some((rc, cond_max));
            continue;
        }
        out.push(op.clone());
    }
    Ok((out, found))
}

pub(super) struct ParsedModalChoice {
    pub(super) min: u8,
    pub(super) raw_max: u8,
    pub(super) uniqueness: ModalUniqueness,
    pub(super) conditional_maximum: Option<(ResolutionCondition, u8)>,
}

pub(super) fn parse_conditional_modal_condition(
    cond_raw: &str,
    compiler_vocab: &spellwake_oracle_compiler::Vocabulary,
) -> Option<ConditionAst> {
    if let Ok(ast) = spellwake_oracle_compiler::filter::parse_condition(cond_raw, compiler_vocab) {
        return Some(ast);
    }
    let norm = cond_raw
        .trim()
        .to_ascii_lowercase()
        .replace("this spell", "~");
    if let Ok(ast) = spellwake_oracle_compiler::filter::parse_condition(&norm, compiler_vocab) {
        return Some(ast);
    }
    if let Some(after_both) = norm.strip_prefix("you control both ")
        && let Some((lhs, rhs)) = after_both.split_once(" and ")
    {
        let expanded = format!(
            "you control {} and if you control {}",
            lhs.trim(),
            rhs.trim()
        );
        if let Ok(ast) =
            spellwake_oracle_compiler::filter::parse_condition(&expanded, compiler_vocab)
        {
            return Some(ast);
        }
    }
    None
}

pub(super) fn unwrap_modal_modes_from_if_body(
    body: &[InstructionAst],
) -> Option<(&str, &[spellwake_oracle_compiler::ast::ModalModeAst])> {
    match body {
        [InstructionAst::Optional(inner)] | [InstructionAst::ReplaceEffect(inner)] => {
            unwrap_modal_modes_from_if_body(inner)
        }
        [InstructionAst::ReplaceEffectWith(rew)] if !rew.only && rew.instead_of.is_none() => {
            unwrap_modal_modes_from_if_body(&rew.body)
        }
        [InstructionAst::Modal { choice, modes }] if modes.len() >= 2 => {
            Some((choice.as_str(), modes.as_slice()))
        }
        _ => None,
    }
}

pub(super) fn normalize_split_conditional_modal(
    ins: &[InstructionAst],
) -> Option<Vec<InstructionAst>> {
    for idx in 0..ins.len().saturating_sub(1) {
        if let InstructionAst::Modal {
            choice: base_choice,
            modes,
        } = &ins[idx]
            && modes.is_empty()
            && let InstructionAst::IfCondition {
                condition,
                then_body,
            } = &ins[idx + 1]
            && let Some((boost_choice, inner_modes)) = unwrap_modal_modes_from_if_body(then_body)
        {
            let mut out = Vec::with_capacity(ins.len());
            out.extend_from_slice(&ins[..idx]);
            out.push(InstructionAst::Modal {
                choice: base_choice.clone(),
                modes: inner_modes.to_vec(),
            });
            out.push(InstructionAst::IfCondition {
                condition: condition.clone(),
                then_body: vec![InstructionAst::ModalModifier(boost_choice.to_string())],
            });
            out.extend_from_slice(&ins[idx + 2..]);
            return Some(out);
        }
    }
    None
}

pub(super) fn parse_modal_choice_with_conditional_max(
    choice: &str,
    modes_len: u8,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ParsedModalChoice, &'static str> {
    let low = choice.to_ascii_lowercase();
    if let Some(idx) = low.find(". if ") {
        let base_raw = choice[..idx].trim();
        let if_tail = choice[idx + ". if ".len()..].trim();
        let (min, raw_max, uniqueness) = parse_modal_choice_with_uniqueness(base_raw)?;
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        for (comma_idx, _) in if_tail.match_indices(", ") {
            let cond_raw = if_tail[..comma_idx].trim();
            let act_raw = if_tail[comma_idx + 2..].trim();
            if let Some(cond_max) = parse_conditional_modal_max_phrase(act_raw, modes_len)
                && let Some(cond_ast) = parse_conditional_modal_condition(cond_raw, &compiler_vocab)
            {
                let rc = lower_world_modal_condition(&cond_ast, vocabulary)
                    .ok_or("oracle:unsupported-conditional-modal-max")?;
                return Ok(ParsedModalChoice {
                    min,
                    raw_max,
                    uniqueness,
                    conditional_maximum: Some((rc, cond_max)),
                });
            }
        }
        return Err("oracle:unsupported-conditional-modal-max");
    }
    let (min, raw_max, uniqueness) = parse_modal_choice_with_uniqueness(choice)?;
    Ok(ParsedModalChoice {
        min,
        raw_max,
        uniqueness,
        conditional_maximum: None,
    })
}

pub(in super::super) fn parse_modal_choice_with_uniqueness(
    choice: &str,
) -> Result<(u8, u8, ModalUniqueness), &'static str> {
    let mut low = choice
        .trim()
        .trim_end_matches('\u{2014}')
        .trim_end_matches('-')
        .trim_end_matches(':')
        .trim()
        .to_ascii_lowercase();
    if low == "one_not_chosen" {
        return Ok((1, 1, ModalUniqueness::OncePerGame));
    }
    for repeat_suffix in [
        ". you may choose the same mode more than once",
        " you may choose the same mode more than once",
        ". each mode must target a different player or permanent",
        " each mode must target a different player or permanent",
        ". each mode must target a different player",
        " each mode must target a different player",
        ". each mode must target a different target",
        " each mode must target a different target",
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
    let base = base
        .strip_suffix(" of the following")
        .unwrap_or(base)
        .trim();
    let (min, max) = parse_modal_choice(base)?;
    Ok((min, max, uniqueness))
}

pub(in super::super) fn parse_modal_choice(choice: &str) -> Result<(u8, u8), &'static str> {
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

pub(in super::super) fn rewrite_value_expr_anaphora(
    expr: &mut ValueExpr,
    replacement: ValueObjectRef,
) {
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

pub(super) fn mana_yield_amount_mut(yield_kind: &mut EffectManaYield) -> Option<&mut ValueExpr> {
    match yield_kind {
        EffectManaYield::RetainedUntilCleanup(inner) => mana_yield_amount_mut(inner),
        EffectManaYield::Counted { amount, .. }
        | EffectManaYield::Split { amount, .. }
        | EffectManaYield::Choice { amount, .. }
        | EffectManaYield::RestrictedChoice { amount, .. }
        | EffectManaYield::LandProducible { amount, .. }
        | EffectManaYield::CommanderIdentity { amount }
        | EffectManaYield::DistinctColors { amount }
        | EffectManaYield::SourceChosenColor { amount } => Some(amount),
        _ => None,
    }
}

pub(super) fn parse_step_name(raw: &str) -> Option<Step> {
    match raw.trim().trim_end_matches('.') {
        "untap" | "untap step" => Some(Step::Untap),
        "upkeep" | "upkeep step" => Some(Step::Upkeep),
        "draw" | "draw step" => Some(Step::Draw),
        "precombat main" | "precombat main phase" | "first main" | "first main phase" => {
            Some(Step::PrecombatMain)
        }
        "beginning of combat" | "beginning of combat step" => Some(Step::BeginningCombat),
        "declare attackers" | "declare attackers step" => Some(Step::DeclareAttackers),
        "declare blockers" | "declare blockers step" => Some(Step::DeclareBlockers),
        "combat damage" | "combat damage step" => Some(Step::CombatDamage),
        "end of combat" | "end of combat step" => Some(Step::EndCombat),
        "postcombat main" | "postcombat main phase" | "second main" | "second main phase" => {
            Some(Step::PostcombatMain)
        }
        "end" | "end step" => Some(Step::End),
        "cleanup" | "cleanup step" => Some(Step::Cleanup),
        _ => None,
    }
}

pub(super) fn split_compound_restriction_clauses(low: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in low.split(['+', ';']) {
        for sent in seg.split(". ") {
            let trimmed = sent.trim().trim_end_matches('.').trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut cursor = trimmed;
            loop {
                let found = [", and only ", " and only ", ", but only ", " but only "]
                    .into_iter()
                    .filter_map(|sep| cursor.find(sep).map(|idx| (idx, sep)))
                    .min_by_key(|&(idx, _)| idx);
                if let Some((idx, sep)) = found {
                    let lhs_t = cursor[..idx].trim();
                    if !lhs_t.is_empty() {
                        parts.push(lhs_t);
                    }
                    cursor = cursor[idx + sep.len()..].trim();
                } else {
                    if !cursor.is_empty() {
                        parts.push(cursor);
                    }
                    break;
                }
            }
        }
    }
    parts
}

pub(in super::super) fn rewrite_instruction_anaphora(
    op: &mut Instruction,
    replacement: ValueObjectRef,
) {
    match op {
        Instruction::AddMana(yield_kind) | Instruction::AddManaForPlayer { yield_kind, .. } => {
            if let Some(amount) = mana_yield_amount_mut(yield_kind) {
                rewrite_value_expr_anaphora(amount, replacement);
            }
        }
        Instruction::Mill { amount, .. }
        | Instruction::Scry { amount, .. }
        | Instruction::Surveil { amount, .. }
        | Instruction::Discard { amount, .. }
        | Instruction::Earthbend { amount }
        | Instruction::Sacrifice { amount, .. }
        | Instruction::ExileTopPlay { count: amount, .. }
        | Instruction::LookTopChoose { count: amount, .. }
        | Instruction::LookTopChooseExt { count: amount, .. }
        | Instruction::RevealUntilMatchExt { count: amount, .. }
        | Instruction::Incubate { count: amount }
        | Instruction::Discover { value: amount }
        | Instruction::Connive { count: amount, .. }
        | Instruction::Blight { amount, .. }
        | Instruction::CollectEvidence {
            minimum: amount, ..
        }
        | Instruction::CreateTokenCopy { count: amount, .. }
        | Instruction::Monstrosity { amount }
        | Instruction::OfferDynamicManaPayment {
            extra_generic: amount,
            ..
        }
        | Instruction::RepeatLoop { count: amount, .. }
        | Instruction::BindResolutionX(amount)
        | Instruction::AffectPermanentTargets {
            action: PermanentTargetAction::PutCounters { amount, .. },
            ..
        } => {
            rewrite_value_expr_anaphora(amount, replacement);
        }
        Instruction::AdjustLife(spec) => match &mut spec.op {
            spellwake_engine::toolbox::LifeOp::Gain(amount)
            | spellwake_engine::toolbox::LifeOp::Lose(amount)
            | spellwake_engine::toolbox::LifeOp::Set(amount)
            | spellwake_engine::toolbox::LifeOp::Pay(amount)
            | spellwake_engine::toolbox::LifeOp::Drain { amount, .. } => {
                rewrite_value_expr_anaphora(amount, replacement);
            }
            spellwake_engine::toolbox::LifeOp::LoseHalf { .. } => {}
        },
        Instruction::DealDamage(spec) => {
            if let spellwake_engine::program::DamageAmountSpec::Expr(expr) = &mut spec.amount {
                rewrite_value_expr_anaphora(expr, replacement);
            }
        }
        Instruction::MutatePlayer(spec) => {
            if let spellwake_engine::toolbox::PlayerMutation::AddCounter { amount, .. }
            | spellwake_engine::toolbox::PlayerMutation::PayCounter { amount, .. } = &mut spec.op
            {
                rewrite_value_expr_anaphora(amount, replacement);
            }
        }
        Instruction::SearchLibrary { filter } => {
            if let SearchCount::UpToExpr(expr) = &mut filter.count {
                rewrite_value_expr_anaphora(expr, replacement);
            }
        }
        Instruction::ZoneOp(spec) => {
            crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_zone_op(
                spec,
                &|_| {},
                &|_| {},
                &|v| rewrite_value_expr_anaphora(v, replacement),
            );
        }
        Instruction::ForEachPlayerQuery { body, .. } | Instruction::ForEach { body, .. } => {
            for inner in &mut body.instructions {
                rewrite_instruction_anaphora(inner, replacement);
            }
        }
        Instruction::PromptChoice(spec) => match &mut spec.domain {
            spellwake_engine::toolbox::PromptDomain::Objects { min, max, .. }
            | spellwake_engine::toolbox::PromptDomain::Number { min, max } => {
                rewrite_value_expr_anaphora(min, replacement);
                rewrite_value_expr_anaphora(max, replacement);
            }
            _ => {}
        },
        Instruction::DrawCards(spec) => {
            rewrite_value_expr_anaphora(&mut spec.amount, replacement);
        }
        Instruction::MutateObject(spec) => {
            match &mut spec.selection {
                spellwake_engine::toolbox::ZoneSelection::Random { count, .. } => {
                    rewrite_value_expr_anaphora(count, replacement);
                }
                spellwake_engine::toolbox::ZoneSelection::Prompt { min, max, .. } => {
                    rewrite_value_expr_anaphora(min, replacement);
                    rewrite_value_expr_anaphora(max, replacement);
                }
                spellwake_engine::toolbox::ZoneSelection::All => {}
            }
            match &mut spec.op {
                spellwake_engine::toolbox::ObjectMutation::AddCounters { amount, .. }
                | spellwake_engine::toolbox::ObjectMutation::RemoveCounters { amount, .. } => {
                    rewrite_value_expr_anaphora(amount, replacement);
                }
                spellwake_engine::toolbox::ObjectMutation::MoveCounters {
                    amount: Some(amount),
                    ..
                } => {
                    rewrite_value_expr_anaphora(amount, replacement);
                }
                _ => {}
            }
        }
        Instruction::ApplyContinuous(spec) => match &mut spec.payload {
            spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts) => {
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
            spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                power, toughness, ..
            } => {
                rewrite_value_expr_anaphora(power, replacement);
                rewrite_value_expr_anaphora(toughness, replacement);
            }
            spellwake_engine::toolbox::ContinuousPayload::SkipTurns { count, .. } => {
                rewrite_value_expr_anaphora(count, replacement);
            }
            _ => {}
        },
        Instruction::CreateTokens { creations }
        | Instruction::CreateAttackingTokens { creations, .. } => {
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

pub(in super::super) fn rewrite_body_anaphora_stats(
    body: &mut ExecutableBody,
    replacement: ValueObjectRef,
) {
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

pub(in super::super) fn rewrite_value_expr_prior_result(
    expr: &mut ValueExpr,
    replacement: EventValueKind,
) {
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

pub(in super::super) fn rewrite_body_prior_result(
    body: &mut ExecutableBody,
    replacement: EventValueKind,
) {
    let mut saw_count_action = false;
    for op in &mut body.instructions {
        if !saw_count_action {
            match op {
                Instruction::AddMana(yield_kind)
                | Instruction::AddManaForPlayer { yield_kind, .. } => {
                    if let Some(amount) = mana_yield_amount_mut(yield_kind) {
                        rewrite_value_expr_prior_result(amount, replacement);
                    }
                }
                Instruction::Mill { amount, .. }
                | Instruction::Scry { amount, .. }
                | Instruction::Surveil { amount, .. }
                | Instruction::Discard { amount, .. }
                | Instruction::Earthbend { amount }
                | Instruction::Sacrifice { amount, .. }
                | Instruction::ExileTopPlay { count: amount, .. }
                | Instruction::LookTopChoose { count: amount, .. }
                | Instruction::LookTopChooseExt { count: amount, .. }
                | Instruction::RevealUntilMatchExt { count: amount, .. }
                | Instruction::BindResolutionX(amount) => {
                    rewrite_value_expr_prior_result(amount, replacement);
                }
                Instruction::AdjustLife(spec) => match &mut spec.op {
                    spellwake_engine::toolbox::LifeOp::Gain(amount)
                    | spellwake_engine::toolbox::LifeOp::Lose(amount)
                    | spellwake_engine::toolbox::LifeOp::Set(amount)
                    | spellwake_engine::toolbox::LifeOp::Pay(amount)
                    | spellwake_engine::toolbox::LifeOp::Drain { amount, .. } => {
                        rewrite_value_expr_prior_result(amount, replacement);
                    }
                    spellwake_engine::toolbox::LifeOp::LoseHalf { .. } => {}
                },
                Instruction::DealDamage(spec) => {
                    if let spellwake_engine::program::DamageAmountSpec::Expr(expr) =
                        &mut spec.amount
                    {
                        rewrite_value_expr_prior_result(expr, replacement);
                    }
                }
                Instruction::MutatePlayer(spec) => {
                    if let spellwake_engine::toolbox::PlayerMutation::AddCounter {
                        amount, ..
                    }
                    | spellwake_engine::toolbox::PlayerMutation::PayCounter {
                        amount, ..
                    } = &mut spec.op
                    {
                        rewrite_value_expr_prior_result(amount, replacement);
                    }
                }
                Instruction::SearchLibrary { filter } => {
                    if let SearchCount::UpToExpr(expr) = &mut filter.count {
                        rewrite_value_expr_prior_result(expr, replacement);
                    }
                }
                Instruction::CreateTokens { creations }
                | Instruction::CreateAttackingTokens { creations, .. } => {
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
                Instruction::ZoneOp(spec) => {
                    crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_zone_op(
                        spec,
                        &|_| {},
                        &|_| {},
                        &|v| rewrite_value_expr_prior_result(v, replacement),
                    );
                }
                Instruction::PromptChoice(spec) => match &mut spec.domain {
                    spellwake_engine::toolbox::PromptDomain::Objects { min, max, .. }
                    | spellwake_engine::toolbox::PromptDomain::Number { min, max } => {
                        rewrite_value_expr_prior_result(min, replacement);
                        rewrite_value_expr_prior_result(max, replacement);
                    }
                    _ => {}
                },
                Instruction::DrawCards(spec) => {
                    rewrite_value_expr_prior_result(&mut spec.amount, replacement);
                }
                Instruction::MutateObject(spec) => {
                    match &mut spec.selection {
                        spellwake_engine::toolbox::ZoneSelection::Random { count, .. } => {
                            rewrite_value_expr_prior_result(count, replacement);
                        }
                        spellwake_engine::toolbox::ZoneSelection::Prompt { min, max, .. } => {
                            rewrite_value_expr_prior_result(min, replacement);
                            rewrite_value_expr_prior_result(max, replacement);
                        }
                        spellwake_engine::toolbox::ZoneSelection::All => {}
                    }
                    match &mut spec.op {
                        spellwake_engine::toolbox::ObjectMutation::AddCounters {
                            amount, ..
                        }
                        | spellwake_engine::toolbox::ObjectMutation::RemoveCounters {
                            amount,
                            ..
                        } => {
                            rewrite_value_expr_prior_result(amount, replacement);
                        }
                        spellwake_engine::toolbox::ObjectMutation::MoveCounters {
                            amount: Some(amount),
                            ..
                        } => {
                            rewrite_value_expr_prior_result(amount, replacement);
                        }
                        _ => {}
                    }
                }
                Instruction::ApplyContinuous(spec) => match &mut spec.payload {
                    spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts) => {
                        for part in &mut parts.0 {
                            match part {
                                ResolvedPartSpec::ModifyPTExpr { power, toughness }
                                | ResolvedPartSpec::SetPTExpr { power, toughness } => {
                                    rewrite_value_expr_prior_result(power, replacement);
                                    rewrite_value_expr_prior_result(toughness, replacement);
                                }
                                _ => {}
                            }
                        }
                    }
                    spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                        power,
                        toughness,
                        ..
                    } => {
                        rewrite_value_expr_prior_result(power, replacement);
                        rewrite_value_expr_prior_result(toughness, replacement);
                    }
                    spellwake_engine::toolbox::ContinuousPayload::SkipTurns { count, .. } => {
                        rewrite_value_expr_prior_result(count, replacement);
                    }
                    _ => {}
                },
                Instruction::ForEachPlayerQuery { body: inner, .. }
                | Instruction::EmitReflexiveTrigger { body: inner }
                | Instruction::ScheduleDelayedStepTrigger { body: inner, .. }
                | Instruction::FixedTargetClause(inner) => {
                    rewrite_body_prior_result(inner, replacement);
                }
                _ => {}
            }
        }
        if matches!(
            op,
            Instruction::ZoneOp(_)
                | Instruction::ForEachPlayerQuery { .. }
                | Instruction::Discard { .. }
                | Instruction::Mill { .. }
                | Instruction::Sacrifice { .. }
        ) {
            saw_count_action = true;
        }
    }
}

pub(in super::super) fn parse_cast_timing_restriction(
    restriction: &str,
    condition: Option<&ConditionAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::program::CastTimingRestriction> {
    use spellwake_engine::program::CastTimingRestriction as R;
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
    let low = restriction
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_ascii_lowercase();
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
    if low == "during combat+during opponent's turn"
        || low == "during opponent's turn+during combat"
    {
        return Some(R::DuringCombatOnOpponentsTurn);
    }
    if (low.contains("declare attackers step") && low.contains("attacked"))
        || low == "duringdeclareattackersifattacked"
    {
        return Some(R::DuringDeclareAttackersIfAttacked);
    }
    let compound = split_compound_restriction_clauses(&low);
    if compound.len() >= 2 {
        let mut parts = Vec::with_capacity(compound.len());
        for part in compound {
            parts.push(parse_cast_timing_restriction(part, None, vocabulary)?);
        }
        return match parts.len() {
            0 => None,
            1 => parts.pop(),
            _ => Some(R::All(parts)),
        };
    }
    let core = low
        .strip_prefix("cast this spell ")
        .or_else(|| low.strip_prefix("cast ~ "))
        .unwrap_or(&low)
        .trim();
    let core = core.strip_prefix("only ").unwrap_or(core).trim();
    if matches!(
        core,
        "xcantbezero" | "x can't be 0" | "x can't be zero" | "x cannot be 0" | "x cannot be zero"
    ) {
        return Some(R::XNonZero);
    }
    if matches!(
        core,
        "aftercombat" | "after combat" | "after combat on your turn"
    ) {
        return Some(R::AfterCombat);
    }
    if matches!(
        core,
        "sorcery" | "as a sorcery" | "any time you could cast a sorcery"
    ) {
        return Some(R::Sorcery);
    }
    if matches!(core, "duringcombat" | "during combat") {
        return Some(R::DuringCombat);
    }
    if (core.contains("declare attackers step") && core.contains("attacked"))
        || core == "duringdeclareattackersifattacked"
    {
        return Some(R::DuringDeclareAttackersIfAttacked);
    }
    if (!core.contains("before combat") && core.contains("before blockers are declared"))
        || core == "duringcombatbeforeblockers"
    {
        return Some(R::DuringCombatBeforeBlockers);
    }
    if core.contains("before the combat damage step")
        || core.contains("before combat damage")
        || core == "beforecombatdamage"
    {
        return Some(R::BeforeCombatDamage);
    }
    if core.contains("after blockers are declared") || core == "duringcombatafterblockers" {
        return Some(R::DuringCombatAfterBlockers);
    }
    if core == "during the declare attackers step" {
        return Some(R::DuringDeclareAttackers);
    }
    if core == "during the declare blockers step" {
        return Some(R::DuringDeclareBlockers);
    }
    if matches!(
        core,
        "during combat on an opponent's turn"
            | "during an opponent's combat"
            | "during an opponent's combat phase"
    ) {
        return Some(R::DuringCombatOnOpponentsTurn);
    }
    if matches!(core, "duringyourturn" | "during your turn") {
        return Some(R::DuringYourTurn);
    }
    if matches!(
        core,
        "duringopponentsturn" | "during an opponent's turn" | "during opponents' turns"
    ) {
        return Some(R::DuringOpponentsTurn);
    }
    if core == "duringupkeep" {
        return Some(R::DuringOwnStep(Step::Upkeep));
    }
    if let Some(step_raw) = core.strip_prefix("during your ")
        && let Some(step) = parse_step_name(step_raw)
    {
        return Some(R::DuringOwnStep(step));
    }
    if let Some(step_raw) = core
        .strip_prefix("during an opponent's ")
        .or_else(|| core.strip_prefix("during opponents' "))
        .or_else(|| core.strip_prefix("during each opponent's "))
        && let Some(step) = parse_step_name(step_raw)
    {
        return Some(R::DuringOpponentsStep(step));
    }
    if let Some(step_raw) = core
        .strip_prefix("during the ")
        .or_else(|| core.strip_prefix("during any player's "))
        .or_else(|| core.strip_prefix("during any "))
        .or_else(|| core.strip_prefix("during each player's "))
        .or_else(|| core.strip_prefix("during each "))
        .or_else(|| core.strip_prefix("during "))
        && let Some(step) = parse_step_name(step_raw)
    {
        return Some(R::DuringStep(step));
    }
    if let Some(cond_str) = core
        .strip_prefix("if ")
        .or_else(|| core.strip_prefix("while "))
    {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        if let Ok(parsed) =
            spellwake_oracle_compiler::filter::parse_condition(cond_str.trim(), &compiler_vocab)
        {
            let mut dummy = BodyBuilder::new();
            if let Ok(rc) = lower_resolution_condition(&parsed, &mut dummy, vocabulary, false, None)
                && dummy.target == BodyTarget::None
                && rc.is_world_evaluable()
            {
                return Some(R::Condition(Box::new(rc)));
            }
        }
    }
    None
}

pub(in super::super) fn strip_triggered_restrictions(
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
        let trimmed = tag.trim().trim_end_matches('.').trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("conditional")
            || trimmed.eq_ignore_ascii_case("condition")
            || trimmed.eq_ignore_ascii_case("only_if")
            || trimmed.eq_ignore_ascii_case("triggers_only_if")
            || trimmed.eq_ignore_ascii_case("if")
        {
            return true;
        }
        let low = trimmed.to_ascii_lowercase();
        let core = low
            .strip_prefix("this ability triggers ")
            .or_else(|| low.strip_prefix("triggers "))
            .or_else(|| low.strip_prefix("trigger "))
            .or_else(|| low.strip_prefix("do this "))
            .unwrap_or(&low)
            .trim();
        let core = core.strip_prefix("only ").unwrap_or(core).trim();
        let push_cond = |acc: &mut Option<TriggerCondition>, next: TriggerCondition| {
            *acc = Some(match acc.take() {
                None => next,
                Some(existing) => TriggerCondition::And(Box::new(existing), Box::new(next)),
            });
        };
        if matches!(
            core,
            "onceeachturn" | "triggersonceeachturn" | "once each turn" | "once per turn"
        ) {
            *grp = TriggerGrouping::OneOrMoreOnceEachTurn;
            *matched_any = true;
            return true;
        }
        if core == "duringyourturnonceeachturn" {
            *grp = TriggerGrouping::OneOrMoreOnceEachTurn;
            push_cond(cond_acc, TriggerCondition::ControllersTurn);
            *matched_any = true;
            return true;
        }
        if matches!(core, "duringyourturn" | "during your turn") {
            push_cond(cond_acc, TriggerCondition::ControllersTurn);
            *matched_any = true;
            return true;
        }
        if matches!(
            core,
            "duringopponentsturn"
                | "during an opponent's turn"
                | "during each opponent's turn"
                | "during opponents' turns"
        ) {
            push_cond(cond_acc, TriggerCondition::OpponentsTurn);
            *matched_any = true;
            return true;
        }
        if core == "duringupkeep" {
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
        if let Some(step_raw) = core.strip_prefix("during your ")
            && let Some(step) = parse_step_name(step_raw)
        {
            push_cond(
                cond_acc,
                TriggerCondition::And(
                    Box::new(TriggerCondition::ControllersTurn),
                    Box::new(TriggerCondition::DuringStep(step)),
                ),
            );
            *matched_any = true;
            return true;
        }
        if let Some(step_raw) = core
            .strip_prefix("during an opponent's ")
            .or_else(|| core.strip_prefix("during opponents' "))
            .or_else(|| core.strip_prefix("during each opponent's "))
            && let Some(step) = parse_step_name(step_raw)
        {
            push_cond(
                cond_acc,
                TriggerCondition::And(
                    Box::new(TriggerCondition::OpponentsTurn),
                    Box::new(TriggerCondition::DuringStep(step)),
                ),
            );
            *matched_any = true;
            return true;
        }
        if let Some(step_raw) = core
            .strip_prefix("during the ")
            .or_else(|| core.strip_prefix("during any player's "))
            .or_else(|| core.strip_prefix("during any "))
            .or_else(|| core.strip_prefix("during each player's "))
            .or_else(|| core.strip_prefix("during each "))
            .or_else(|| core.strip_prefix("during "))
            && let Some(step) = parse_step_name(step_raw)
        {
            push_cond(cond_acc, TriggerCondition::DuringStep(step));
            *matched_any = true;
            return true;
        }
        if matches!(core, "duringcombat" | "during combat") {
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
        if matches!(core, "khans" | "if khans was chosen" | "entry_option:0") {
            push_cond(cond_acc, TriggerCondition::SourceEntryOption(0));
            *matched_any = true;
            return true;
        }
        if matches!(core, "dragons" | "if dragons was chosen" | "entry_option:1") {
            push_cond(cond_acc, TriggerCondition::SourceEntryOption(1));
            *matched_any = true;
            return true;
        }
        if let Some(opt_raw) = core.strip_prefix("entry_option:")
            && let Ok(opt_idx) = opt_raw.trim().parse::<u8>()
        {
            push_cond(cond_acc, TriggerCondition::SourceEntryOption(opt_idx));
            *matched_any = true;
            return true;
        }
        if let Some(cond_str) = core
            .strip_prefix("if ")
            .or_else(|| core.strip_prefix("while "))
        {
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            if let Ok(parsed) =
                spellwake_oracle_compiler::filter::parse_condition(cond_str.trim(), &compiler_vocab)
                && let Ok(tc) = lower_trigger_condition(&parsed, vocabulary)
            {
                push_cond(cond_acc, tc);
                *matched_any = true;
                return true;
            }
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
            let base_low = base_raw.trim().to_ascii_lowercase();
            let clauses = split_compound_restriction_clauses(&base_low);
            let tags_ok = !clauses.is_empty()
                && clauses.iter().all(|part| {
                    apply_trig_tag(part, &mut local_cond, &mut local_grp, &mut matched_any)
                });
            if tags_ok || (base_low.is_empty() && condition.is_some()) {
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
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::Optional(s));
                }
                continue;
            }
            InstructionAst::IfPreviousSucceeded(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::IfPreviousSucceeded(s));
                }
                continue;
            }
            InstructionAst::IfPreviousFailed(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::IfPreviousFailed(s));
                }
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
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::OptionalPlayer {
                        player: *player,
                        body: s,
                    });
                }
                continue;
            }
            InstructionAst::Otherwise(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::Otherwise(s));
                }
                continue;
            }
            InstructionAst::ReplaceEffect(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::ReplaceEffect(s));
                }
                continue;
            }
            InstructionAst::ReplaceEffectWith(rew) => {
                let s = strip_triggered_restrictions(&rew.body, spec, grouping, vocabulary);
                if !(s.is_empty() && !rew.body.is_empty()) {
                    stripped_body.push(InstructionAst::ReplaceEffectWith(Box::new(
                        spellwake_oracle_compiler::ast::ReplaceEffectWithAst {
                            body: s,
                            instead_of: rew.instead_of.clone(),
                            only: rew.only,
                        },
                    )));
                }
                continue;
            }
            InstructionAst::ReflexiveTrigger(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::ReflexiveTrigger(s));
                }
                continue;
            }
            InstructionAst::Choice(branches) => {
                let new_branches = branches
                    .iter()
                    .map(|br| strip_triggered_restrictions(br, spec, grouping, vocabulary))
                    .collect();
                stripped_body.push(InstructionAst::Choice(new_branches));
                continue;
            }
            InstructionAst::DelayedTrigger {
                step,
                scope,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::DelayedTrigger {
                    step: *step,
                    scope: *scope,
                    body: s,
                });
                continue;
            }
            InstructionAst::DelayedEventTrigger {
                trigger,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::DelayedEventTrigger {
                    trigger: trigger.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::SpreeMode { cost, body: inner } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::SpreeMode {
                    cost: cost.clone(),
                    body: s,
                });
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
                let new_action = unless_action
                    .as_ref()
                    .map(|act| strip_triggered_restrictions(act, spec, grouping, vocabulary));
                stripped_body.push(InstructionAst::Unless {
                    body: s,
                    unless_player: *unless_player,
                    unless_cost: unless_cost.clone(),
                    unless_action: new_action,
                    unless_condition: unless_condition.clone(),
                });
                continue;
            }
            InstructionAst::Modal { choice, modes } => {
                let new_modes = modes
                    .iter()
                    .map(|m| spellwake_oracle_compiler::ast::ModalModeAst {
                        body: strip_triggered_restrictions(&m.body, spec, grouping, vocabulary),
                        label: m.label.clone(),
                        granted: m.granted.clone(),
                    })
                    .collect();
                stripped_body.push(InstructionAst::Modal {
                    choice: choice.clone(),
                    modes: new_modes,
                });
                continue;
            }
            InstructionAst::BindXY {
                x_expr,
                y_expr,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::BindXY {
                    x_expr: x_expr.clone(),
                    y_expr: y_expr.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::Simultaneous(inner) => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::Simultaneous(s));
                }
                continue;
            }
            InstructionAst::ForEachCategory {
                category,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForEachCategory {
                    category: category.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::ForEachSubjectPlayer {
                players,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForEachSubjectPlayer {
                    players: players.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::ForEachColorAmong {
                source,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForEachColorAmong {
                    source: source.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::ForAsLongAs {
                condition,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ForAsLongAs {
                    condition: condition.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::WhenYouNext {
                event,
                body: inner,
                duration,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::WhenYouNext {
                    event: event.clone(),
                    body: s,
                    duration: duration.clone(),
                });
                continue;
            }
            InstructionAst::TemporaryTrigger {
                trigger,
                body: inner,
                duration,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::TemporaryTrigger {
                    trigger: trigger.clone(),
                    body: s,
                    duration: duration.clone(),
                });
                continue;
            }
            InstructionAst::TemporarySpecialAction {
                timing,
                duration,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::TemporarySpecialAction {
                    timing: timing.clone(),
                    duration: duration.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::ReflexiveEventTrigger {
                trigger,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ReflexiveEventTrigger {
                    trigger: trigger.clone(),
                    body: s,
                });
                continue;
            }
            InstructionAst::ManaSpendRider { cast_filter, rider } => {
                let s = strip_triggered_restrictions(rider, spec, grouping, vocabulary);
                stripped_body.push(InstructionAst::ManaSpendRider {
                    cast_filter: cast_filter.clone(),
                    rider: s,
                });
                continue;
            }
            InstructionAst::SequentialChoose { body: inner, .. } if !inner.is_empty() => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                let mut cloned = ins.clone();
                if let InstructionAst::SequentialChoose {
                    body: ref mut slot, ..
                } = cloned
                {
                    *slot = s;
                }
                stripped_body.push(cloned);
                continue;
            }
            InstructionAst::OneShotReplacement {
                instead_body: Some(inner),
                ..
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                let mut cloned = ins.clone();
                if let InstructionAst::OneShotReplacement {
                    instead_body: ref mut slot,
                    ..
                } = cloned
                {
                    *slot = Some(s);
                }
                stripped_body.push(cloned);
                continue;
            }
            InstructionAst::Counter {
                unless_action: Some(inner),
                ..
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                let mut cloned = ins.clone();
                if let InstructionAst::Counter {
                    unless_action: ref mut slot,
                    ..
                } = cloned
                {
                    *slot = Some(s);
                }
                stripped_body.push(cloned);
                continue;
            }
            InstructionAst::SearchLibrary { follow_up, .. } if !follow_up.is_empty() => {
                let s = strip_triggered_restrictions(follow_up, spec, grouping, vocabulary);
                let mut cloned = ins.clone();
                if let InstructionAst::SearchLibrary {
                    follow_up: ref mut slot,
                    ..
                } = cloned
                {
                    *slot = s;
                }
                stripped_body.push(cloned);
                continue;
            }
            InstructionAst::ScopedDuration {
                duration,
                body: inner,
            } => {
                let s = strip_triggered_restrictions(inner, spec, grouping, vocabulary);
                if !(s.is_empty() && !inner.is_empty()) {
                    stripped_body.push(InstructionAst::ScopedDuration {
                        duration: duration.clone(),
                        body: s,
                    });
                }
                continue;
            }
            _ => {}
        }
        stripped_body.push(ins.clone());
    }
    stripped_body
}

pub(in super::super) fn body_returns_self_from_graveyard(body: &[InstructionAst]) -> bool {
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

pub(in super::super) fn body_references_self_from_graveyard(body: &[InstructionAst]) -> bool {
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

pub(super) fn rewrite_value_expr_trigger_subject(
    expr: &mut ValueExpr,
    replacement: ValueObjectRef,
) {
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

pub(super) fn rewrite_body_trigger_subject_stats(
    body: &mut ExecutableBody,
    replacement: ValueObjectRef,
) {
    for op in &mut body.instructions {
        match op {
            Instruction::AddMana(yield_kind) | Instruction::AddManaForPlayer { yield_kind, .. } => {
                if let Some(amount) = mana_yield_amount_mut(yield_kind) {
                    rewrite_value_expr_trigger_subject(amount, replacement);
                }
            }
            Instruction::Mill { amount, .. }
            | Instruction::Scry { amount, .. }
            | Instruction::Surveil { amount, .. }
            | Instruction::Discard { amount, .. }
            | Instruction::Earthbend { amount }
            | Instruction::Sacrifice { amount, .. }
            | Instruction::ExileTopPlay { count: amount, .. }
            | Instruction::LookTopChoose { count: amount, .. }
            | Instruction::Incubate { count: amount }
            | Instruction::Discover { value: amount }
            | Instruction::Connive { count: amount, .. }
            | Instruction::Blight { amount, .. }
            | Instruction::CollectEvidence {
                minimum: amount, ..
            }
            | Instruction::CreateTokenCopy { count: amount, .. }
            | Instruction::Monstrosity { amount }
            | Instruction::OfferDynamicManaPayment {
                extra_generic: amount,
                ..
            }
            | Instruction::RepeatLoop { count: amount, .. }
            | Instruction::BindResolutionX(amount)
            | Instruction::AffectPermanentTargets {
                action: PermanentTargetAction::PutCounters { amount, .. },
                ..
            } => {
                rewrite_value_expr_trigger_subject(amount, replacement);
            }
            Instruction::AdjustLife(spec) => match &mut spec.op {
                spellwake_engine::toolbox::LifeOp::Gain(amount)
                | spellwake_engine::toolbox::LifeOp::Lose(amount)
                | spellwake_engine::toolbox::LifeOp::Set(amount)
                | spellwake_engine::toolbox::LifeOp::Pay(amount)
                | spellwake_engine::toolbox::LifeOp::Drain { amount, .. } => {
                    rewrite_value_expr_trigger_subject(amount, replacement);
                }
                spellwake_engine::toolbox::LifeOp::LoseHalf { .. } => {}
            },
            Instruction::DealDamage(spec) => {
                if let spellwake_engine::program::DamageAmountSpec::Expr(expr) = &mut spec.amount {
                    rewrite_value_expr_trigger_subject(expr, replacement);
                }
            }
            Instruction::MutatePlayer(spec) => {
                if let spellwake_engine::toolbox::PlayerMutation::AddCounter { amount, .. }
                | spellwake_engine::toolbox::PlayerMutation::PayCounter { amount, .. } =
                    &mut spec.op
                {
                    rewrite_value_expr_trigger_subject(amount, replacement);
                }
            }
            Instruction::SearchLibrary { filter } => {
                if let SearchCount::UpToExpr(expr) = &mut filter.count {
                    rewrite_value_expr_trigger_subject(expr, replacement);
                }
            }
            Instruction::ZoneOp(spec) => {
                crate::keyword_creatures::oracle_lowering::toolbox_emit::rewrite_zone_op(
                    spec,
                    &|_| {},
                    &|_| {},
                    &|v| rewrite_value_expr_trigger_subject(v, replacement),
                );
            }
            Instruction::ForEachPlayerQuery { body, .. } | Instruction::ForEach { body, .. } => {
                rewrite_body_trigger_subject_stats(body, replacement);
            }
            Instruction::PromptChoice(spec) => match &mut spec.domain {
                spellwake_engine::toolbox::PromptDomain::Objects { min, max, .. }
                | spellwake_engine::toolbox::PromptDomain::Number { min, max } => {
                    rewrite_value_expr_trigger_subject(min, replacement);
                    rewrite_value_expr_trigger_subject(max, replacement);
                }
                _ => {}
            },
            Instruction::DrawCards(spec) => {
                rewrite_value_expr_trigger_subject(&mut spec.amount, replacement);
            }
            Instruction::MutateObject(spec) => {
                match &mut spec.selection {
                    spellwake_engine::toolbox::ZoneSelection::Random { count, .. } => {
                        rewrite_value_expr_trigger_subject(count, replacement);
                    }
                    spellwake_engine::toolbox::ZoneSelection::Prompt { min, max, .. } => {
                        rewrite_value_expr_trigger_subject(min, replacement);
                        rewrite_value_expr_trigger_subject(max, replacement);
                    }
                    spellwake_engine::toolbox::ZoneSelection::All => {}
                }
                match &mut spec.op {
                    spellwake_engine::toolbox::ObjectMutation::AddCounters { amount, .. }
                    | spellwake_engine::toolbox::ObjectMutation::RemoveCounters {
                        amount, ..
                    } => {
                        rewrite_value_expr_trigger_subject(amount, replacement);
                    }
                    spellwake_engine::toolbox::ObjectMutation::MoveCounters {
                        amount: Some(amount),
                        ..
                    } => {
                        rewrite_value_expr_trigger_subject(amount, replacement);
                    }
                    _ => {}
                }
            }
            Instruction::ApplyContinuous(spec) => match &mut spec.payload {
                spellwake_engine::toolbox::ContinuousPayload::ResolvedParts(parts) => {
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
                spellwake_engine::toolbox::ContinuousPayload::DynamicBoost {
                    power,
                    toughness,
                    ..
                } => {
                    rewrite_value_expr_trigger_subject(power, replacement);
                    rewrite_value_expr_trigger_subject(toughness, replacement);
                }
                spellwake_engine::toolbox::ContinuousPayload::SkipTurns { count, .. } => {
                    rewrite_value_expr_trigger_subject(count, replacement);
                }
                _ => {}
            },
            Instruction::CreateTokens { creations }
            | Instruction::CreateAttackingTokens { creations, .. } => {
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
            | Instruction::ScheduleDelayedStepTrigger { body: inner, .. }
            | Instruction::FixedTargetClause(inner) => {
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

pub(in super::super) fn apply_trigger_body_rewrites(
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
        if let Some(prefix) = modes.prefix.as_deref_mut() {
            apply_one(prefix);
        }
        if let Some(suffix) = modes.suffix.as_deref_mut() {
            apply_one(suffix);
        }
        for opt in &mut modes.options {
            apply_one(opt);
        }
    }
}

pub(in super::super) fn combine_activation_restrictions(
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

pub(super) fn is_instant_only_restriction(raw: &str) -> bool {
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

pub(super) fn is_still_type_restriction_str(raw: &str) -> bool {
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

pub(super) fn parse_only_spend_mana_type_restriction(raw: &str) -> Option<ManaType> {
    let low = raw.trim().trim_end_matches('.').trim().to_ascii_lowercase();
    if let Some(color_tok) = low
        .strip_prefix("only_spend_")
        .or_else(|| low.strip_prefix("onlyspend"))
        .or_else(|| low.strip_prefix("spendonly"))
    {
        return parse_color_word_mana_type(color_tok.trim());
    }
    // Strictly match activation-cost mana restrictions ("spend only black mana on X"),
    // never produced-mana restrictions ("spend this mana only to cast ...").
    let rest = low.strip_prefix("spend only ")?.trim();
    let trimmed = rest
        .strip_suffix(" on x")
        .or_else(|| rest.strip_suffix(" to activate this ability"))
        .or_else(|| rest.strip_suffix(" to activate ~"))
        .or_else(|| rest.strip_suffix(" to activate it"))
        .or_else(|| rest.strip_suffix(" this way"))
        .unwrap_or(rest)
        .trim();
    if let Some(color_part) = trimmed.strip_suffix(" mana") {
        return parse_color_word_mana_type(color_part.trim());
    }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return parse_color_word_mana_type(trimmed);
    }
    None
}

pub(super) fn is_legendary_creature_or_planeswalker_cost_reduction(
    delta: &str,
    per: &ObjectFilterAst,
) -> bool {
    let delta_clean = delta.trim();
    if delta_clean != "{1}" {
        return false;
    }
    let want_types = AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER);
    per.any_types == want_types
        && per.required_supertypes == AstSupertypeMask::LEGENDARY
        && per.forbidden_types.is_empty()
        && per.forbidden_supertypes.is_empty()
        && per.subtypes.is_empty()
        && per.forbidden_subtypes.is_empty()
        && per.any_colors.is_empty()
        && per.forbidden_colors.is_empty()
        && per.color_arity.is_none()
        && matches!(per.controller, None | Some(ControllerRelation::You))
        && per.owner.is_none()
        && per.zone.is_none()
        && per.extra_zones.is_empty()
        && per.combat.is_none()
        && per.tapped.is_none()
        && per.token.is_none()
        && per.required_keywords.is_empty()
        && per.forbidden_keywords.is_empty()
        && per.stat_bounds.is_empty()
        && per.counter_kind.is_none()
        && per.named.is_none()
        && per.flags.is_empty()
        && per.predicates.is_empty()
}

pub(super) fn parse_activation_restriction_with_vocab(
    raw: &str,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ActivationRestriction, &'static str> {
    if let Ok(r) = parse_activation_restriction(raw) {
        return Ok(r);
    }
    let trimmed = raw.trim().trim_end_matches('.').trim();
    if let Some(rest) = trimmed
        .strip_prefix("AnyPlayer:")
        .or_else(|| trimmed.strip_prefix("anyplayer:"))
    {
        let rest_trimmed = rest.trim();
        if rest_trimmed.is_empty() {
            return Ok(ActivationRestriction::AnyPlayer);
        }
        let inner = parse_activation_restriction_with_vocab(rest_trimmed, vocabulary)?;
        return Ok(combine_activation_restrictions(
            Some(ActivationRestriction::AnyPlayer),
            inner,
        ));
    }
    let low = trimmed.to_ascii_lowercase();
    let clauses = split_compound_restriction_clauses(&low);
    if clauses.len() >= 2 {
        let mut acc: Option<ActivationRestriction> = None;
        for part in clauses {
            if part.is_empty() || is_instant_only_restriction(part) {
                continue;
            }
            let next_r = parse_activation_restriction_with_vocab(part, vocabulary)?;
            acc = Some(combine_activation_restrictions(acc, next_r));
        }
        return acc.ok_or("oracle:unsupported-act-restriction");
    }
    let core = low
        .strip_prefix("activate this ability ")
        .or_else(|| low.strip_prefix("activate "))
        .or_else(|| low.strip_prefix("use this ability "))
        .or_else(|| low.strip_prefix("do this "))
        .unwrap_or(&low)
        .trim();
    let core = core.strip_prefix("only ").unwrap_or(core).trim();
    if let Some(cond_str) = core
        .strip_prefix("if ")
        .or_else(|| core.strip_prefix("while "))
    {
        let compiler_vocab = cached_compiler_vocab(vocabulary);
        if let Ok(parsed) =
            spellwake_oracle_compiler::filter::parse_condition(cond_str.trim(), &compiler_vocab)
        {
            return lower_conditional_activation_restriction(&parsed, vocabulary);
        }
    }
    Err("oracle:unsupported-act-restriction")
}

pub(super) fn is_activation_restriction_text(raw: &str) -> bool {
    let trimmed = raw.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() || is_still_type_restriction_str(trimmed) {
        return false;
    }
    if matches!(
        trimmed,
        "ExcessDamageToController"
            | "DamageCantBePreventedThisTurn"
            | "DamageCantBePrevented"
            | "NoMaxHandSizeRestOfGame"
            | "FlashbackEqualManaCost"
    ) {
        return false;
    }
    let low = trimmed.to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "sorcery"
            | "onceeachturn"
            | "triggersonceeachturn"
            | "duringupkeep"
            | "ownturnbeforeattackers"
            | "exhaust"
            | "once"
            | "duringyourturn"
            | "duringopponentsturn"
            | "duringcombat"
            | "duringdeclareattackersstep"
            | "duringdeclareblockersstep"
            | "duringbeginningofcombatstep"
            | "duringcombatdamagestep"
            | "duringendofcombatstep"
            | "duringyourmainphase"
            | "main phase"
            | "sorceryonceeachturn"
            | "duringyourturnonceeachturn"
            | "anyplayer"
            | "onlyopponents"
            | "xcantbezero"
            | "x can't be 0"
            | "x can't be zero"
            | "x cannot be 0"
            | "x cannot be zero"
            | "khans"
            | "dragons"
            | "if khans was chosen"
            | "if dragons was chosen"
            | "as a sorcery"
            | "any time you could cast a sorcery"
    ) {
        return true;
    }
    if low.starts_with("activate ")
        || low.starts_with("only ")
        || low.starts_with("any player ")
        || low.starts_with("anyplayer")
        || low.starts_with("spend only ")
        || low.starts_with("only_spend_")
        || low.starts_with("onlyspend")
        || low.starts_with("spendonly")
        || low.starts_with("x can't be 0")
        || low.starts_with("x can't be zero")
        || low.starts_with("x cannot be 0")
        || low.starts_with("x cannot be zero")
        || low.starts_with("use this ability ")
        || low.starts_with("do this ")
        || low.starts_with("this ability triggers ")
        || low.starts_with("triggers ")
        || low.starts_with("entry_option:")
        || low.starts_with("during ")
        || (low.starts_with("atmost") && low.ends_with("timeseachturn"))
        || parse_max_per_turn_activation(&low).is_some()
    {
        return true;
    }
    let clauses = split_compound_restriction_clauses(&low);
    if clauses.len() >= 2 {
        let mut iter = clauses
            .into_iter()
            .filter(|c| !c.is_empty() && !is_instant_only_restriction(c));
        if let Some(first) = iter.next() {
            return is_activation_restriction_text(first)
                && iter.all(|c| {
                    is_activation_restriction_text(c)
                        || c.starts_with("if ")
                        || c.starts_with("while ")
                });
        }
    }
    false
}

pub(in super::super) fn strip_activated_restrictions(
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
            } if is_instant_only_restriction(r) => {
                // Normal ActivatedAbility already enforces instant-speed priority timing.
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: None,
            } if is_activation_restriction_text(r) => {
                let next_r = parse_activation_restriction_with_vocab(r, vocabulary)?;
                restriction = Some(combine_activation_restrictions(restriction, next_r));
            }
            InstructionAst::AbilityRestriction {
                condition: None, ..
            } => {
                filtered.push(item.clone());
            }
            InstructionAst::AbilityRestriction {
                restriction: r,
                condition: Some(cond),
            } => {
                let base = r.strip_prefix("Conditional").unwrap_or(r.as_str()).trim();
                if !base.is_empty()
                    && !base.eq_ignore_ascii_case("condition")
                    && !base.eq_ignore_ascii_case("only_if")
                    && !base.eq_ignore_ascii_case("triggers_only_if")
                    && !base.eq_ignore_ascii_case("if")
                    && !is_instant_only_restriction(base)
                {
                    let base_r = parse_activation_restriction_with_vocab(base, vocabulary)?;
                    restriction = Some(combine_activation_restrictions(restriction, base_r));
                }
                let next_r = lower_conditional_activation_restriction(cond, vocabulary)?;
                restriction = Some(combine_activation_restrictions(restriction, next_r));
            }
            InstructionAst::ManaSpendRestriction(r)
                if parse_only_spend_mana_type_restriction(r).is_some() =>
            {
                if let Some(mt) = parse_only_spend_mana_type_restriction(r) {
                    restriction = Some(combine_activation_restrictions(
                        restriction,
                        ActivationRestriction::OnlySpendManaType(mt),
                    ));
                }
            }
            InstructionAst::AbilityCostReduction { delta, per }
                if is_legendary_creature_or_planeswalker_cost_reduction(delta, per) =>
            {
                restriction = Some(combine_activation_restrictions(
                    restriction,
                    ActivationRestriction::DiscountLegendaryCreaturesAndPlaneswalkers,
                ));
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
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Optional(inner_filtered));
                }
            }
            InstructionAst::IfPreviousSucceeded(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::IfPreviousSucceeded(inner_filtered));
                }
            }
            InstructionAst::IfPreviousFailed(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::IfPreviousFailed(inner_filtered));
                }
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
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::OptionalPlayer {
                        player: *player,
                        body: inner_filtered,
                    });
                }
            }
            InstructionAst::Otherwise(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Otherwise(inner_filtered));
                }
            }
            InstructionAst::ReplaceEffect(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ReplaceEffect(inner_filtered));
                }
            }
            InstructionAst::ReplaceEffectWith(rew) => {
                let (inner_filtered, inner_r) =
                    strip_activated_restrictions(&rew.body, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !rew.body.is_empty()) {
                    filtered.push(InstructionAst::ReplaceEffectWith(Box::new(
                        spellwake_oracle_compiler::ast::ReplaceEffectWithAst {
                            body: inner_filtered,
                            instead_of: rew.instead_of.clone(),
                            only: rew.only,
                        },
                    )));
                }
            }
            InstructionAst::ReflexiveTrigger(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ReflexiveTrigger(inner_filtered));
                }
            }
            InstructionAst::Choice(branches) => {
                let mut new_branches = Vec::with_capacity(branches.len());
                for br in branches {
                    let (bf, br_r) = strip_activated_restrictions(br, vocabulary)?;
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
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
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
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::DelayedEventTrigger {
                    trigger: trigger.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::SpreeMode { cost, body: inner } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
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
                let new_action = match unless_action {
                    Some(act) => {
                        let (af, ar) = strip_activated_restrictions(act, vocabulary)?;
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
                    let (mb, mr) = strip_activated_restrictions(&m.body, vocabulary)?;
                    if let Some(ir) = mr {
                        restriction = Some(combine_activation_restrictions(restriction, ir));
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
            InstructionAst::BindXY {
                x_expr,
                y_expr,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::BindXY {
                    x_expr: x_expr.clone(),
                    y_expr: y_expr.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::Simultaneous(inner) => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::Simultaneous(inner_filtered));
                }
            }
            InstructionAst::ForEachCategory {
                category,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachCategory {
                    category: category.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachSubjectPlayer {
                players,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachSubjectPlayer {
                    players: players.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForEachColorAmong {
                source,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForEachColorAmong {
                    source: source.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ForAsLongAs {
                condition,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ForAsLongAs {
                    condition: condition.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::WhenYouNext {
                event,
                body: inner,
                duration,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::WhenYouNext {
                    event: event.clone(),
                    body: inner_filtered,
                    duration: duration.clone(),
                });
            }
            InstructionAst::TemporaryTrigger {
                trigger,
                body: inner,
                duration,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::TemporaryTrigger {
                    trigger: trigger.clone(),
                    body: inner_filtered,
                    duration: duration.clone(),
                });
            }
            InstructionAst::TemporarySpecialAction {
                timing,
                duration,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::TemporarySpecialAction {
                    timing: timing.clone(),
                    duration: duration.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ReflexiveEventTrigger {
                trigger,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ReflexiveEventTrigger {
                    trigger: trigger.clone(),
                    body: inner_filtered,
                });
            }
            InstructionAst::ManaSpendRider { cast_filter, rider } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(rider, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                filtered.push(InstructionAst::ManaSpendRider {
                    cast_filter: cast_filter.clone(),
                    rider: inner_filtered,
                });
            }
            InstructionAst::SequentialChoose { body: inner, .. } if !inner.is_empty() => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                let mut cloned = item.clone();
                if let InstructionAst::SequentialChoose {
                    body: ref mut slot, ..
                } = cloned
                {
                    *slot = inner_filtered;
                }
                filtered.push(cloned);
            }
            InstructionAst::OneShotReplacement {
                instead_body: Some(inner),
                ..
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                let mut cloned = item.clone();
                if let InstructionAst::OneShotReplacement {
                    instead_body: ref mut slot,
                    ..
                } = cloned
                {
                    *slot = Some(inner_filtered);
                }
                filtered.push(cloned);
            }
            InstructionAst::Counter {
                unless_action: Some(inner),
                ..
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                let mut cloned = item.clone();
                if let InstructionAst::Counter {
                    unless_action: ref mut slot,
                    ..
                } = cloned
                {
                    *slot = Some(inner_filtered);
                }
                filtered.push(cloned);
            }
            InstructionAst::SearchLibrary { follow_up, .. } if !follow_up.is_empty() => {
                let (inner_filtered, inner_r) =
                    strip_activated_restrictions(follow_up, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                let mut cloned = item.clone();
                if let InstructionAst::SearchLibrary {
                    follow_up: ref mut slot,
                    ..
                } = cloned
                {
                    *slot = inner_filtered;
                }
                filtered.push(cloned);
            }
            InstructionAst::ScopedDuration {
                duration,
                body: inner,
            } => {
                let (inner_filtered, inner_r) = strip_activated_restrictions(inner, vocabulary)?;
                if let Some(ir) = inner_r {
                    restriction = Some(combine_activation_restrictions(restriction, ir));
                }
                if !(inner_filtered.is_empty() && !inner.is_empty()) {
                    filtered.push(InstructionAst::ScopedDuration {
                        duration: duration.clone(),
                        body: inner_filtered,
                    });
                }
            }
            _ => {
                filtered.push(item.clone());
            }
        }
    }
    Ok((filtered, restriction))
}

pub(in super::super) fn lower_conditional_activation_restriction(
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
            value,
        } if *value > 0 => {
            let plain_extras = filter.forbidden_types.is_empty()
                && filter.required_supertypes.is_empty()
                && filter.forbidden_supertypes.is_empty()
                && filter.forbidden_subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.color_arity.is_none()
                && filter.controller.is_none()
                && filter.owner.is_none()
                && filter.zone.is_none()
                && filter.extra_zones.is_empty()
                && filter.combat.is_none()
                && filter.tapped.is_none()
                && filter.token.is_none()
                && filter.required_keywords.is_empty()
                && filter.forbidden_keywords.is_empty()
                && filter.counter_kind.is_none()
                && filter.named.is_none()
                && filter.flags.is_empty()
                && filter.predicates.is_empty();
            if *value == 1
                && plain_extras
                && filter.any_types == AstCardTypeMask::CREATURE
                && filter.subtypes.is_empty()
                && filter.stat_bounds.len() == 1
                && filter.stat_bounds[0].stat == StatKind::Power
                && filter.stat_bounds[0].op == CompareOp::Ge
                && matches!(filter.stat_bounds[0].value.as_ref(), ValueExprAst::Const(4))
            {
                return Ok(ActivationRestriction::ControlsCreaturePowerFourOrGreater);
            }
            let min_u8 = u8::try_from(*value).map_err(|_| "oracle:unsupported-act-restriction")?;
            if plain_extras && filter.stat_bounds.is_empty() {
                if filter.subtypes.len() == 1
                    && (filter.any_types.is_empty()
                        || filter.any_types == AstCardTypeMask::CREATURE)
                    && let Some(ct) = vocabulary.resolve_label(&filter.subtypes[0])
                {
                    let minimum =
                        NonZeroU8::new(min_u8).ok_or("oracle:unsupported-act-restriction")?;
                    return Ok(ActivationRestriction::ControlsSubtypeCount { kind: ct, minimum });
                }
                if filter.subtypes.is_empty() {
                    let mut types = card_type_mask_to_set(filter.any_types)?;
                    if types.len() == 1
                        && let Some(kind) = types.pop_first()
                    {
                        return Ok(ActivationRestriction::ControlsCardType {
                            kind,
                            minimum: min_u8,
                        });
                    }
                }
            }
            let obj = lower_object_filter(filter, vocabulary)?;
            Ok(ActivationRestriction::ControlsMatching {
                filter: obj,
                minimum: min_u8,
            })
        }
        ConditionAst::Threshold => Ok(ActivationRestriction::Threshold),
        ConditionAst::Delirium => Ok(ActivationRestriction::Delirium),
        ConditionAst::Metalcraft => Ok(ActivationRestriction::Metalcraft),
        ConditionAst::IsYourTurn | ConditionAst::IsPlayersTurn(PlayerRefAst::You) => {
            Ok(ActivationRestriction::DuringYourTurn)
        }
        ConditionAst::IsPlayersTurn(PlayerRefAst::AnyOpponent | PlayerRefAst::EachOpponent) => {
            Ok(ActivationRestriction::DuringOpponentsTurn)
        }
        ConditionAst::Not(inner)
            if matches!(
                inner.as_ref(),
                ConditionAst::IsYourTurn | ConditionAst::IsPlayersTurn(PlayerRefAst::You)
            ) =>
        {
            Ok(ActivationRestriction::DuringOpponentsTurn)
        }
        ConditionAst::And(a, b) => {
            let ra = lower_conditional_activation_restriction(a, vocabulary)?;
            let rb = lower_conditional_activation_restriction(b, vocabulary)?;
            Ok(combine_activation_restrictions(Some(ra), rb))
        }
        ConditionAst::HasStatus { subject, status }
            if matches!(
                unwrap_battlefield_subject(subject),
                SubjectRefAst::SelfObject
            ) || matches!(
                unwrap_battlefield_subject(subject),
                SubjectRefAst::Anaphora(a) if is_self_anaphora(a)
            ) =>
        {
            match status.trim().to_ascii_lowercase().as_str() {
                "tapped" => Ok(ActivationRestriction::SourceIsTapped),
                "untapped" => Ok(ActivationRestriction::SourceIsUntapped),
                "attacking" => Ok(ActivationRestriction::SourceMatches(
                    ObjectFilter::any().attacking(),
                )),
                "blocking" => Ok(ActivationRestriction::SourceMatches(
                    ObjectFilter::any().combat(CombatFilter::Blocking),
                )),
                "attacking or blocking" => Ok(ActivationRestriction::SourceMatches(
                    ObjectFilter::any().attacking_or_blocking(),
                )),
                "unblocked" => Ok(ActivationRestriction::SourceMatches(
                    ObjectFilter::any().combat(CombatFilter::Unblocked),
                )),
                "equipped" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::EQUIPPED);
                    Ok(ActivationRestriction::SourceMatches(ObjectFilter::Clause(
                        c,
                    )))
                }
                "enchanted" => {
                    let mut c = ObjectClause::any();
                    c.flags = c.flags.union(ObjectFilterFlags::ENCHANTED);
                    Ok(ActivationRestriction::SourceMatches(ObjectFilter::Clause(
                        c,
                    )))
                }
                _ => {
                    let mut tmp_builder = BodyBuilder::new();
                    let rc =
                        lower_resolution_condition(cond, &mut tmp_builder, vocabulary, false, None)
                            .map_err(|_| "oracle:unsupported-act-restriction")?;
                    if tmp_builder.target != BodyTarget::None || !rc.is_world_evaluable() {
                        return Err("oracle:unsupported-act-restriction");
                    }
                    Ok(ActivationRestriction::Condition(Box::new(rc)))
                }
            }
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

pub(super) fn parse_max_per_turn_activation(core: &str) -> Option<ActivationRestriction> {
    let before_turn = core
        .strip_suffix(" each turn")
        .or_else(|| core.strip_suffix(" per turn"))?
        .trim();
    let (explicit_bound, body) = if let Some(rest) = before_turn
        .strip_prefix("no more than ")
        .or_else(|| before_turn.strip_prefix("at most "))
        .or_else(|| before_turn.strip_prefix("up to "))
    {
        (true, rest.trim())
    } else {
        (false, before_turn)
    };
    let (has_times_suffix, num_tok) = if let Some(rest) = body
        .strip_suffix(" times")
        .or_else(|| body.strip_suffix(" time"))
    {
        (true, rest.trim())
    } else {
        (false, body)
    };
    let count: u8 = match num_tok {
        "once" => 1,
        "twice" => 2,
        "thrice" => 3,
        "one" | "1" if explicit_bound || has_times_suffix => 1,
        "two" | "2" if explicit_bound || has_times_suffix => 2,
        "three" | "3" if explicit_bound || has_times_suffix => 3,
        "four" | "4" if explicit_bound || has_times_suffix => 4,
        "five" | "5" if explicit_bound || has_times_suffix => 5,
        "six" | "6" if explicit_bound || has_times_suffix => 6,
        "seven" | "7" if explicit_bound || has_times_suffix => 7,
        "eight" | "8" if explicit_bound || has_times_suffix => 8,
        "nine" | "9" if explicit_bound || has_times_suffix => 9,
        "ten" | "10" if explicit_bound || has_times_suffix => 10,
        other if explicit_bound || has_times_suffix => {
            other.parse::<u8>().ok().filter(|&v| v > 0)?
        }
        _ => return None,
    };
    if count == 1 {
        Some(ActivationRestriction::OnceEachTurn)
    } else {
        Some(ActivationRestriction::MaxPerTurn(count))
    }
}

pub(in super::super) fn parse_activation_restriction(
    raw: &str,
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
            parse_activation_restriction(rest_trimmed)?
        };
        return Ok(combine_activation_restrictions(
            Some(ActivationRestriction::AnyPlayer),
            inner,
        ));
    }
    let low = trimmed.to_ascii_lowercase();
    let clauses = split_compound_restriction_clauses(&low);
    if clauses.len() >= 2 {
        let mut acc: Option<ActivationRestriction> = None;
        for part in clauses {
            if part.is_empty() || is_instant_only_restriction(part) {
                continue;
            }
            let next_r = parse_activation_restriction(part)?;
            acc = Some(combine_activation_restrictions(acc, next_r));
        }
        return acc.ok_or("oracle:unsupported-act-restriction");
    }
    match low.as_str() {
        "sorcery" => return Ok(ActivationRestriction::Sorcery),
        "onceeachturn" | "triggersonceeachturn" => return Ok(ActivationRestriction::OnceEachTurn),
        "duringupkeep" => return Ok(ActivationRestriction::DuringUpkeep),
        "ownturnbeforeattackers" => return Ok(ActivationRestriction::OwnTurnBeforeAttackers),
        "exhaust" | "once" => return Ok(ActivationRestriction::Exhaust),
        "duringyourturn" => return Ok(ActivationRestriction::DuringYourTurn),
        "duringopponentsturn" => return Ok(ActivationRestriction::DuringOpponentsTurn),
        "duringcombat" => return Ok(ActivationRestriction::DuringCombat),
        "duringdeclareattackersstep" => {
            return Ok(ActivationRestriction::DuringStep(
                spellwake_engine::turn::Step::DeclareAttackers,
            ));
        }
        "duringdeclareblockersstep" => {
            return Ok(ActivationRestriction::DuringStep(
                spellwake_engine::turn::Step::DeclareBlockers,
            ));
        }
        "duringbeginningofcombatstep" => {
            return Ok(ActivationRestriction::DuringStep(
                spellwake_engine::turn::Step::BeginningCombat,
            ));
        }
        "duringcombatdamagestep" => {
            return Ok(ActivationRestriction::DuringStep(
                spellwake_engine::turn::Step::CombatDamage,
            ));
        }
        "duringendofcombatstep" => {
            return Ok(ActivationRestriction::DuringStep(
                spellwake_engine::turn::Step::EndCombat,
            ));
        }
        "sorceryonceeachturn" => return Ok(ActivationRestriction::SorceryOnceEachTurn),
        "duringyourturnonceeachturn" => {
            return Ok(ActivationRestriction::DuringYourTurnOnceEachTurn);
        }
        "anyplayer" => return Ok(ActivationRestriction::AnyPlayer),
        "onlyopponents" => return Ok(ActivationRestriction::OnlyOpponents),
        "xcantbezero" | "x can't be 0" | "x can't be zero" | "x cannot be 0"
        | "x cannot be zero" => {
            return Ok(ActivationRestriction::XNonZero);
        }
        "khans" | "if khans was chosen" | "entry_option:0" => {
            return Ok(ActivationRestriction::SourceEntryOption(0));
        }
        "dragons" | "if dragons was chosen" | "entry_option:1" => {
            return Ok(ActivationRestriction::SourceEntryOption(1));
        }
        _ => {}
    }
    if let Some(mid) = low
        .strip_prefix("atmost")
        .and_then(|rest| rest.strip_suffix("timeseachturn"))
        && let Ok(n) = mid.parse::<u8>()
        && n >= 1
    {
        return Ok(if n == 1 {
            ActivationRestriction::OnceEachTurn
        } else {
            ActivationRestriction::MaxPerTurn(n)
        });
    }
    if let Some(opt_raw) = low.strip_prefix("entry_option:")
        && let Ok(opt_idx) = opt_raw.trim().parse::<u8>()
    {
        return Ok(ActivationRestriction::SourceEntryOption(opt_idx));
    }
    if let Some(mt) = parse_only_spend_mana_type_restriction(&low) {
        return Ok(ActivationRestriction::OnlySpendManaType(mt));
    }
    if is_instant_only_restriction(&low) {
        return Err("oracle:instant-restriction-handled-above");
    }
    let core = low
        .strip_prefix("activate this ability ")
        .or_else(|| low.strip_prefix("activate "))
        .or_else(|| low.strip_prefix("use this ability "))
        .or_else(|| low.strip_prefix("this ability triggers "))
        .or_else(|| low.strip_prefix("triggers "))
        .or_else(|| low.strip_prefix("do this "))
        .unwrap_or(&low)
        .trim();
    let core = core.strip_prefix("only ").unwrap_or(core).trim();
    if matches!(
        low.as_str(),
        "only your opponents may activate this ability"
            | "only an opponent may activate this ability"
            | "only any opponent may activate this ability"
            | "only opponents may activate this ability"
    ) {
        return Ok(ActivationRestriction::OnlyOpponents);
    }
    if low == "any player may activate this ability" {
        return Ok(ActivationRestriction::AnyPlayer);
    }
    let has_once = core.contains("once each turn") || core.contains("once per turn");
    if core.contains("sorcery") && has_once {
        Ok(ActivationRestriction::SorceryOnceEachTurn)
    } else if core.contains("during your turn") && has_once {
        Ok(ActivationRestriction::DuringYourTurnOnceEachTurn)
    } else if core.contains("during your turn") && core.contains("before attackers") {
        Ok(ActivationRestriction::OwnTurnBeforeAttackers)
    } else if matches!(
        core,
        "sorcery" | "as a sorcery" | "any time you could cast a sorcery"
    ) {
        Ok(ActivationRestriction::Sorcery)
    } else if let Some(per_turn) = parse_max_per_turn_activation(core) {
        Ok(per_turn)
    } else if has_once {
        Ok(ActivationRestriction::OnceEachTurn)
    } else if matches!(core, "exhaust" | "once") || low == "activate only once" {
        Ok(ActivationRestriction::Exhaust)
    } else if matches!(core, "during your combat" | "during your combat phase") {
        Ok(ActivationRestriction::All(vec![
            ActivationRestriction::DuringYourTurn,
            ActivationRestriction::DuringCombat,
        ]))
    } else if matches!(
        core,
        "during an opponent's combat"
            | "during an opponent's combat phase"
            | "during combat on an opponent's turn"
    ) {
        Ok(ActivationRestriction::All(vec![
            ActivationRestriction::DuringOpponentsTurn,
            ActivationRestriction::DuringCombat,
        ]))
    } else if matches!(
        core,
        "main phase"
            | "during main phase"
            | "duringyourmainphase"
            | "during your main phase"
            | "during your main phases"
            | "during either of your main phases"
            | "during one of your main phases"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::IsOwnMainPhase),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(1),
            },
        )))
    } else if let Some(step_raw) = core.strip_prefix("during your ")
        && let Some(step) = parse_step_name(step_raw)
    {
        if step == Step::Upkeep {
            Ok(ActivationRestriction::DuringUpkeep)
        } else {
            Ok(ActivationRestriction::DuringOwnStep(step))
        }
    } else if let Some(step_raw) = core
        .strip_prefix("during an opponent's ")
        .or_else(|| core.strip_prefix("during opponents' "))
        .or_else(|| core.strip_prefix("during each opponent's "))
        && let Some(step) = parse_step_name(step_raw)
    {
        Ok(ActivationRestriction::DuringOpponentsStep(step))
    } else if let Some(step_raw) = core
        .strip_prefix("during the ")
        .or_else(|| core.strip_prefix("during any player's "))
        .or_else(|| core.strip_prefix("during any "))
        .or_else(|| core.strip_prefix("during each player's "))
        .or_else(|| core.strip_prefix("during each "))
        .or_else(|| core.strip_prefix("during "))
        && let Some(step) = parse_step_name(step_raw)
    {
        Ok(ActivationRestriction::DuringStep(step))
    } else if matches!(core, "duringyourturn" | "during your turn") {
        Ok(ActivationRestriction::DuringYourTurn)
    } else if matches!(
        core,
        "duringopponentsturn"
            | "during an opponent's turn"
            | "during each opponent's turn"
            | "during opponents' turns"
    ) {
        Ok(ActivationRestriction::DuringOpponentsTurn)
    } else if matches!(
        core,
        "duringcombat" | "during combat" | "during combat phase" | "during the combat phase"
    ) {
        Ok(ActivationRestriction::DuringCombat)
    } else if matches!(
        core,
        "if ~ is untapped"
            | "if this permanent is untapped"
            | "if this creature is untapped"
            | "if this artifact is untapped"
            | "if this land is untapped"
            | "while ~ is untapped"
    ) {
        Ok(ActivationRestriction::SourceIsUntapped)
    } else if matches!(
        core,
        "if ~ is tapped"
            | "if this permanent is tapped"
            | "if this creature is tapped"
            | "if this artifact is tapped"
            | "if this land is tapped"
            | "while ~ is tapped"
    ) {
        Ok(ActivationRestriction::SourceIsTapped)
    } else if matches!(core, "if ~ is attacking" | "if this creature is attacking") {
        Ok(ActivationRestriction::SourceMatches(
            ObjectFilter::any().attacking(),
        ))
    } else if matches!(core, "if ~ is blocking" | "if this creature is blocking") {
        Ok(ActivationRestriction::SourceMatches(
            ObjectFilter::any().combat(CombatFilter::Blocking),
        ))
    } else if matches!(
        core,
        "if ~ is attacking or blocking" | "if this creature is attacking or blocking"
    ) {
        Ok(ActivationRestriction::SourceMatches(
            ObjectFilter::any().attacking_or_blocking(),
        ))
    } else if matches!(core, "if ~ is unblocked" | "if this creature is unblocked") {
        Ok(ActivationRestriction::SourceMatches(
            ObjectFilter::any().combat(CombatFilter::Unblocked),
        ))
    } else if matches!(
        core,
        "if ~ is equipped" | "if this creature is equipped" | "while ~ is equipped"
    ) {
        let mut c = ObjectClause::any();
        c.flags = c.flags.union(ObjectFilterFlags::EQUIPPED);
        Ok(ActivationRestriction::SourceMatches(ObjectFilter::Clause(
            c,
        )))
    } else if matches!(
        core,
        "if ~ is enchanted"
            | "if this creature is enchanted"
            | "if this permanent is enchanted"
            | "while ~ is enchanted"
    ) {
        let mut c = ObjectClause::any();
        c.flags = c.flags.union(ObjectFilterFlags::ENCHANTED);
        Ok(ActivationRestriction::SourceMatches(ObjectFilter::Clause(
            c,
        )))
    } else if matches!(
        core,
        "if ~ attacked this turn" | "if this creature attacked this turn"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::AttackedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(
        core,
        "if ~ is in your graveyard"
            | "if this card is in your graveyard"
            | "while ~ is in your graveyard"
            | "while this card is in your graveyard"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(
                    spellwake_engine::program::SourceZoneKind::Graveyard,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(
        core,
        "while ~ is on the battlefield"
            | "if ~ is on the battlefield"
            | "while this permanent is on the battlefield"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::SourceState(SourceStateExpr::InZone(
                    spellwake_engine::program::SourceZoneKind::Battlefield,
                )),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(core, "if you have the city's blessing") {
        Ok(ActivationRestriction::HasCityBlessing)
    } else if matches!(
        core,
        "if you created a token this turn" | "if you've created a token this turn"
    ) {
        Ok(ActivationRestriction::CreatedTokenThisTurn)
    } else if matches!(
        core,
        "if you control a creature with power 4 or greater"
            | "if you control a creature with power four or greater"
    ) {
        Ok(ActivationRestriction::ControlsCreaturePowerFourOrGreater)
    } else if matches!(
        core,
        "if seven or more cards are in your graveyard"
            | "if you have seven or more cards in your graveyard"
    ) {
        Ok(ActivationRestriction::Threshold)
    } else if matches!(
        core,
        "if there are four or more card types among cards in your graveyard"
    ) {
        Ok(ActivationRestriction::Delirium)
    } else if matches!(core, "if you control three or more artifacts") {
        Ok(ActivationRestriction::Metalcraft)
    } else if matches!(core, "if you haven't cast a spell this turn") {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            },
        )))
    } else if matches!(
        core,
        "if you cast a spell this turn" | "if you've cast a spell this turn"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerSpellsCastThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(
        core,
        "if you gained life this turn" | "if you've gained life this turn"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeGainedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(
        core,
        "if you lost life this turn" | "if you've lost life this turn"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(
        core,
        "if an opponent lost life this turn" | "if an opponent has lost life this turn"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::OpponentsLifeLostThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(
        core,
        "if you attacked this turn" | "if you attacked with a creature this turn"
    ) {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::PlayerStat(PlayerStatExpr::ControllerAttackedThisTurn),
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(1),
            },
        )))
    } else if matches!(core, "if you control no other creatures") {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::count_player_battlefield(
                    ResolvingPlayer::Controller,
                    ObjectFilter::Clause(ObjectClause {
                        source_relation: SourceRelation::OtherThanAbilitySource,
                        ..ObjectClause::creature()
                    }),
                ),
                comparison: Comparison::Equal,
                right: ValueExpr::literal(0),
            },
        )))
    } else if let Some(rest) = core
        .strip_prefix("if creatures you control have total power ")
        .and_then(|r| r.strip_suffix(" or greater"))
        && let Some(n) = parse_small_positive_count(rest)
    {
        Ok(ActivationRestriction::Condition(Box::new(
            ResolutionCondition {
                left: ValueExpr::Aggregate {
                    filter: Box::new(ObjectFilter::creature().controlled()),
                    stat: ObjectStatKind::Power,
                    op: StatAggregate::Sum,
                },
                comparison: Comparison::GreaterOrEqual,
                right: ValueExpr::literal(n),
            },
        )))
    } else if let Some(rest) = core
        .strip_prefix("if you have ")
        .and_then(|r| r.strip_suffix(" or more life"))
        && let Ok(n) = rest.trim().parse::<u16>()
    {
        Ok(ActivationRestriction::ControllerLifeAtLeast(n))
    } else if let Some(rest) = core
        .strip_prefix("if you have ")
        .and_then(|r| r.strip_suffix(" or less life"))
        && let Ok(n) = rest.trim().parse::<u16>()
    {
        Ok(ActivationRestriction::ControllerLifeAtMost(n))
    } else if let Some(after_has) = [
        "if it has ",
        "if ~ has ",
        "if this land has ",
        "if this permanent has ",
        "if this artifact has ",
        "if this creature has ",
        "if this enchantment has ",
    ]
    .iter()
    .find_map(|p| core.strip_prefix(p))
        && let Some(counter_clause) = after_has
            .strip_suffix(" on it")
            .or_else(|| after_has.strip_suffix(" on ~"))
    {
        if counter_clause == "no counters" {
            Ok(ActivationRestriction::Condition(Box::new(
                ResolutionCondition {
                    left: ValueExpr::CounterCount {
                        subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                        kind: None,
                    },
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                },
            )))
        } else if let Some(kind_raw) = counter_clause.strip_prefix("no ").and_then(|r| {
            r.strip_suffix(" counters")
                .or_else(|| r.strip_suffix(" counter"))
        }) && let Some(ck) = parse_counter_kind(kind_raw.trim())
            && ck.is_permanent()
        {
            Ok(ActivationRestriction::Condition(Box::new(
                ResolutionCondition {
                    left: ValueExpr::CounterCount {
                        subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                        kind: Some(ck),
                    },
                    comparison: Comparison::Equal,
                    right: ValueExpr::literal(0),
                },
            )))
        } else if let Some((cnt_raw, rest_kind)) = counter_clause.split_once(" or more ")
            && let Some(n) = parse_small_positive_count(cnt_raw.trim())
            && let Some(kind_raw) = rest_kind
                .strip_suffix(" counters")
                .or_else(|| rest_kind.strip_suffix(" counter"))
            && let Some(ck) = parse_counter_kind(kind_raw.trim())
            && ck.is_permanent()
        {
            Ok(ActivationRestriction::Condition(Box::new(
                ResolutionCondition {
                    left: ValueExpr::CounterCount {
                        subject: spellwake_engine::program::CounterSubjectRef::AbilitySource,
                        kind: Some(ck),
                    },
                    comparison: Comparison::GreaterOrEqual,
                    right: ValueExpr::literal(n),
                },
            )))
        } else {
            Err("oracle:unsupported-act-restriction")
        }
    } else if low.contains("creature died this turn") {
        Ok(ActivationRestriction::CreatureDiedThisTurn)
    } else if low.contains("no cards in hand") || low.contains("no cards in your hand") {
        Ok(ActivationRestriction::EmptyHand)
    } else {
        Err("oracle:unsupported-act-restriction")
    }
}
