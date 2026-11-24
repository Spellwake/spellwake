use super::*;

pub(super) fn subject_is_anaphora(subj: &SubjectRefAst) -> bool {
    matches!(subj, SubjectRefAst::Anaphora(_))
}

pub(super) fn player_is_target_anaphora(p: PlayerRefAst) -> bool {
    matches!(
        p,
        PlayerRefAst::ThatPlayer
            | PlayerRefAst::ItsController
            | PlayerRefAst::ThatObjectController
            | PlayerRefAst::ItsOwner
    )
}

pub(super) fn value_expr_references_target_anaphora(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ObjectStat { subject, .. } | ValueExprAst::CounterCount { subject, .. } => {
            subject_is_anaphora(subject)
        }
        ValueExprAst::PlayerStat { player, .. }
        | ValueExprAst::PlayerCounterCount { player, .. }
        | ValueExprAst::LifeChangedThisTurn { player, .. } => player_is_target_anaphora(*player),
        ValueExprAst::Add(a, b) | ValueExprAst::Sub(a, b) | ValueExprAst::Mul(a, b) => {
            value_expr_references_target_anaphora(a) || value_expr_references_target_anaphora(b)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_target_anaphora(expr),
        _ => false,
    }
}

pub(super) fn condition_references_target_anaphora(cond: &ConditionAst) -> bool {
    match cond {
        ConditionAst::Not(inner) => condition_references_target_anaphora(inner),
        ConditionAst::And(a, b) | ConditionAst::Or(a, b) => {
            condition_references_target_anaphora(a) || condition_references_target_anaphora(b)
        }
        ConditionAst::Compare { lhs, rhs, .. } => {
            value_expr_references_target_anaphora(lhs) || value_expr_references_target_anaphora(rhs)
        }
        ConditionAst::MatchesFilter { subject, .. } | ConditionAst::HasStatus { subject, .. } => {
            subject_is_anaphora(subject)
        }
        ConditionAst::ControlsCount { player, .. }
        | ConditionAst::HandCount { player, .. }
        | ConditionAst::GraveyardCount { player, .. }
        | ConditionAst::LifeCompare { player, .. }
        | ConditionAst::PlayerActionThisTurn { player, .. } => player_is_target_anaphora(*player),
        _ => false,
    }
}

pub(super) fn instruction_references_target_anaphora(instr: &InstructionAst) -> bool {
    if instruction_references_anaphora(instr) {
        return true;
    }
    match instr {
        InstructionAst::ChangeLife { player, delta, .. } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(delta)
        }
        InstructionAst::SetLifeTotal { player, value } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(value)
        }
        InstructionAst::Draw { player, count }
        | InstructionAst::Mill { player, count }
        | InstructionAst::Scry { player, count }
        | InstructionAst::Surveil { player, count }
        | InstructionAst::Investigate { player, count }
        | InstructionAst::Blight { player, count } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(count)
        }
        InstructionAst::Discard { player, count, .. } => {
            player_is_target_anaphora(*player)
                || count
                    .as_deref()
                    .is_some_and(value_expr_references_target_anaphora)
        }
        InstructionAst::DiscardHand { player }
        | InstructionAst::RevealHand { player }
        | InstructionAst::ShuffleLibrary { player }
        | InstructionAst::ExtraTurn { player }
        | InstructionAst::BecomeMonarch { player }
        | InstructionAst::TakeInitiative { player } => player_is_target_anaphora(*player),
        InstructionAst::Sacrifice { player, count, .. } => {
            player_is_target_anaphora(*player)
                || count
                    .as_deref()
                    .is_some_and(value_expr_references_target_anaphora)
        }
        InstructionAst::DealDamage { amount, .. } => value_expr_references_target_anaphora(amount),
        InstructionAst::PutCounters { count, .. } => value_expr_references_target_anaphora(count),
        InstructionAst::RemoveCounters { count, .. } => count
            .as_deref()
            .is_some_and(value_expr_references_target_anaphora),
        InstructionAst::GainPlayerCounters { player, count, .. } => {
            player_is_target_anaphora(*player) || value_expr_references_target_anaphora(count)
        }
        InstructionAst::CreateToken { controller, .. } => player_is_target_anaphora(*controller),
        InstructionAst::ModifyPT { scale, .. } => scale
            .as_deref()
            .is_some_and(value_expr_references_target_anaphora),
        InstructionAst::PlayerPayCost { player, .. } => player_is_target_anaphora(*player),
        InstructionAst::SearchLibrary { library, .. } => matches!(
            library.trim().to_ascii_lowercase().as_str(),
            "that player" | "that player's" | "its controller" | "its controller's"
        ),
        InstructionAst::ExileTopLibrary { actor, whose, .. } => {
            actor.is_some_and(player_is_target_anaphora)
                || matches!(
                    whose.trim().to_ascii_lowercase().as_str(),
                    "that player" | "that player's" | "its controller" | "its controller's"
                )
        }
        InstructionAst::IfCondition {
            condition,
            then_body,
        } => {
            condition_references_target_anaphora(condition)
                || then_body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::Optional(inner)
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::DelayedEventTrigger { body: inner, .. } => {
            inner.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::OptionalPlayer { player, body } => {
            player_is_target_anaphora(*player)
                || body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::Unless {
            body,
            unless_player,
            unless_action,
            unless_condition,
            ..
        } => {
            unless_player.is_some_and(player_is_target_anaphora)
                || unless_condition
                    .as_deref()
                    .is_some_and(condition_references_target_anaphora)
                || body.iter().any(instruction_references_target_anaphora)
                || unless_action
                    .as_deref()
                    .is_some_and(|a| a.iter().any(instruction_references_target_anaphora))
        }
        _ => false,
    }
}

pub(super) fn instruction_references_anaphora(instr: &InstructionAst) -> bool {
    match instr {
        InstructionAst::DealDamage { source, target, .. } => {
            subject_is_anaphora(source) || subject_is_anaphora(target)
        }
        InstructionAst::Fight { a, b } => subject_is_anaphora(a) || subject_is_anaphora(b),
        InstructionAst::MoveCounters { from, to, .. } => {
            subject_is_anaphora(from) || subject_is_anaphora(to)
        }
        InstructionAst::Attach { subject, target } => {
            subject_is_anaphora(subject) || subject_is_anaphora(target)
        }
        InstructionAst::RedirectDamage { from, to, .. } => {
            subject_is_anaphora(from) || subject_is_anaphora(to)
        }
        InstructionAst::BecomeCopy {
            subject, source, ..
        } => subject_is_anaphora(subject) || subject_is_anaphora(source),
        InstructionAst::Exchange { left, right, .. } => {
            subject_is_anaphora(left) || right.as_deref().is_some_and(subject_is_anaphora)
        }
        InstructionAst::PutCounters { subject, .. }
        | InstructionAst::RemoveCounters { subject, .. }
        | InstructionAst::Destroy { subject, .. }
        | InstructionAst::Exile { subject, .. }
        | InstructionAst::ExileUntilLeaves { subject }
        | InstructionAst::Sacrifice { subject, .. }
        | InstructionAst::SacrificeUnlessPay { subject, .. }
        | InstructionAst::ReturnZone { subject, .. }
        | InstructionAst::MoveZone { subject, .. }
        | InstructionAst::TapPermanent { subject }
        | InstructionAst::UntapPermanent { subject }
        | InstructionAst::TapOrUntapPermanent { subject }
        | InstructionAst::SkipNextUntap { subject }
        | InstructionAst::ModifyPT { subject, .. }
        | InstructionAst::ModifyPTChoice { subject, .. }
        | InstructionAst::SwitchPowerToughness { subject, .. }
        | InstructionAst::GrantKeywords { subject, .. }
        | InstructionAst::RemoveKeywords { subject, .. }
        | InstructionAst::RemoveAllAbilities { subject, .. }
        | InstructionAst::GrantAbilityText { subject, .. }
        | InstructionAst::GainControl { subject, .. }
        | InstructionAst::PhaseOut { subject }
        | InstructionAst::PhaseOutUntil { subject, .. }
        | InstructionAst::Transform { subject }
        | InstructionAst::Regenerate { subject }
        | InstructionAst::Copy { subject, .. }
        | InstructionAst::GrantCastPermission { subject, .. }
        | InstructionAst::CombatRuleMod { subject, .. }
        | InstructionAst::AnimateOrSetBasePT { subject, .. }
        | InstructionAst::SetDynamicPT { subject, .. }
        | InstructionAst::Explore { subject }
        | InstructionAst::Detain { subject }
        | InstructionAst::Suspect { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::Exert { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::DoubleStatOrCounters { subject, .. } => subject_is_anaphora(subject),
        InstructionAst::Counter { target, .. }
        | InstructionAst::PreventDamage { target, .. }
        | InstructionAst::ChangeTargets { target }
        | InstructionAst::ChangeText { target, .. } => subject_is_anaphora(target),
        InstructionAst::PreventAllDamage { target, .. } => {
            target.as_deref().is_some_and(subject_is_anaphora)
        }
        InstructionAst::Cloak { subject, .. } | InstructionAst::Airbend { subject } => {
            subject.as_deref().is_some_and(subject_is_anaphora)
        }
        InstructionAst::Discard { anaphora, .. } => anaphora.is_some(),
        InstructionAst::Optional(inner)
        | InstructionAst::OptionalPlayer { body: inner, .. }
        | InstructionAst::IfPreviousSucceeded(inner)
        | InstructionAst::IfPreviousFailed(inner)
        | InstructionAst::Otherwise(inner)
        | InstructionAst::ReplaceEffect(inner)
        | InstructionAst::ReflexiveTrigger(inner)
        | InstructionAst::BindX { body: inner, .. }
        | InstructionAst::IfCondition {
            then_body: inner, ..
        }
        | InstructionAst::ForEachPlayer { body: inner, .. }
        | InstructionAst::ForEachObject { body: inner, .. }
        | InstructionAst::ForEachCount { body: inner, .. }
        | InstructionAst::DelayedTrigger { body: inner, .. }
        | InstructionAst::DelayedEventTrigger { body: inner, .. } => {
            inner.iter().any(instruction_references_anaphora)
        }
        InstructionAst::Unless {
            body,
            unless_action,
            ..
        } => {
            body.iter().any(instruction_references_anaphora)
                || unless_action
                    .as_ref()
                    .is_some_and(|a| a.iter().any(instruction_references_anaphora))
        }
        InstructionAst::Choice(branches) => branches
            .iter()
            .any(|b| b.iter().any(instruction_references_anaphora)),
        _ => false,
    }
}

pub(super) fn scale_ast_amount(base: &ValueExprAst, mult: ValueExprAst) -> ValueExprAst {
    if matches!(base, ValueExprAst::Const(1)) {
        mult
    } else {
        ValueExprAst::Mul(Box::new(mult), Box::new(base.clone()))
    }
}

pub(super) fn try_scale_token_spec(
    spec: &TokenSpecAst,
    mult: ValueExprAst,
) -> Option<TokenSpecAst> {
    match spec {
        TokenSpecAst::Predefined {
            name,
            count,
            tapped,
        } => Some(TokenSpecAst::Predefined {
            name: name.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
            tapped: *tapped,
        }),
        TokenSpecAst::Creature {
            count,
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        } => Some(TokenSpecAst::Creature {
            count: Box::new(scale_ast_amount(count, mult)),
            power: power.clone(),
            toughness: toughness.clone(),
            colors: *colors,
            supertypes: *supertypes,
            card_types: *card_types,
            subtypes: subtypes.clone(),
            keywords: keywords.clone(),
            tapped: *tapped,
            attacking: *attacking,
            named: named.clone(),
        }),
        _ => None,
    }
}

pub(super) fn try_scale_instruction(
    instr: &InstructionAst,
    mult: ValueExprAst,
) -> Option<InstructionAst> {
    match instr {
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } => Some(InstructionAst::ChangeLife {
            player: *player,
            delta: Box::new(scale_ast_amount(delta, mult)),
            gain: *gain,
        }),
        InstructionAst::Draw { player, count } => Some(InstructionAst::Draw {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::Mill { player, count } => Some(InstructionAst::Mill {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora: None,
            random: false,
        } => Some(InstructionAst::Discard {
            player: *player,
            count: Some(Box::new(scale_ast_amount(
                count.as_deref().unwrap_or(&ValueExprAst::Const(1)),
                mult,
            ))),
            filter: filter.clone(),
            anaphora: None,
            random: false,
        }),
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } => Some(InstructionAst::DealDamage {
            source: source.clone(),
            target: target.clone(),
            amount: Box::new(scale_ast_amount(amount, mult)),
        }),
        InstructionAst::PutCounters {
            subject,
            counter_kind,
            count,
        } => Some(InstructionAst::PutCounters {
            subject: subject.clone(),
            counter_kind: counter_kind.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } => Some(InstructionAst::GainPlayerCounters {
            player: *player,
            counter_kind: counter_kind.clone(),
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        InstructionAst::CreateToken { controller, token } => Some(InstructionAst::CreateToken {
            controller: *controller,
            token: try_scale_token_spec(token, mult)?,
        }),
        InstructionAst::Investigate { player, count } => Some(InstructionAst::Investigate {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
        }),
        _ => None,
    }
}

pub(super) fn instruction_acts_only_on_controller(instr: &InstructionAst) -> bool {
    matches!(
        instr,
        InstructionAst::Scry {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Surveil {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Proliferate
            | InstructionAst::Explore { .. }
            | InstructionAst::FlipCoin
            | InstructionAst::RollDie { .. }
            | InstructionAst::Investigate {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::CreateToken {
                controller: PlayerRefAst::You,
                ..
            }
            | InstructionAst::Draw {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::ChangeLife {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::Mill {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::GainPlayerCounters {
                player: PlayerRefAst::You,
                ..
            }
            | InstructionAst::PutCounters { .. }
    )
}

pub(super) fn try_scale_controller_instruction(
    instr: &InstructionAst,
    mult: ValueExprAst,
) -> Option<InstructionAst> {
    let acts_on_you = matches!(
        instr,
        InstructionAst::ChangeLife {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Draw {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::Mill {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::GainPlayerCounters {
            player: PlayerRefAst::You,
            ..
        } | InstructionAst::CreateToken {
            controller: PlayerRefAst::You,
            ..
        } | InstructionAst::Investigate {
            player: PlayerRefAst::You,
            ..
        }
    );
    if acts_on_you {
        try_scale_instruction(instr, mult)
    } else {
        None
    }
}

pub(super) fn try_rewrite_for_each_player(
    instr: &InstructionAst,
    target_player: PlayerRefAst,
) -> Option<InstructionAst> {
    let matches_loop_player = |p: PlayerRefAst| p == PlayerRefAst::ThatPlayer || p == target_player;
    match instr {
        InstructionAst::Draw { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Draw {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Mill { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Mill {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Discard {
            player,
            count,
            filter,
            anaphora,
            random,
        } if matches_loop_player(*player) => Some(InstructionAst::Discard {
            player: target_player,
            count: count.clone(),
            filter: filter.clone(),
            anaphora: anaphora.clone(),
            random: *random,
        }),
        InstructionAst::Sacrifice {
            player,
            count,
            subject,
        } if matches_loop_player(*player) => Some(InstructionAst::Sacrifice {
            player: target_player,
            count: count.clone(),
            subject: subject.clone(),
        }),
        InstructionAst::ChangeLife {
            player,
            delta,
            gain,
        } if matches_loop_player(*player) => Some(InstructionAst::ChangeLife {
            player: target_player,
            delta: delta.clone(),
            gain: *gain,
        }),
        InstructionAst::CreateToken { controller, token } if matches_loop_player(*controller) => {
            Some(InstructionAst::CreateToken {
                controller: target_player,
                token: token.clone(),
            })
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } if matches_loop_player(*player) => Some(InstructionAst::GainPlayerCounters {
            player: target_player,
            counter_kind: counter_kind.clone(),
            count: count.clone(),
        }),
        InstructionAst::Investigate { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Investigate {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Scry { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Scry {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::Surveil { player, count } if matches_loop_player(*player) => {
            Some(InstructionAst::Surveil {
                player: target_player,
                count: count.clone(),
            })
        }
        InstructionAst::DealDamage {
            source,
            target,
            amount,
        } if matches!(target.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) => {
            Some(InstructionAst::DealDamage {
                source: source.clone(),
                target: Box::new(SubjectRefAst::Player(target_player)),
                amount: amount.clone(),
            })
        }
        InstructionAst::GainPlayerCounters {
            player,
            counter_kind,
            count,
        } if matches_loop_player(*player) => Some(InstructionAst::GainPlayerCounters {
            player: target_player,
            counter_kind: counter_kind.clone(),
            count: count.clone(),
        }),
        InstructionAst::ExileGraveyard { whose }
            if matches!(
                whose.trim().to_ascii_lowercase().as_str(),
                "their" | "that player's" | "his or her"
            ) =>
        {
            let mapped = match target_player {
                PlayerRefAst::EachPlayer => "each player's",
                PlayerRefAst::EachOpponent => "each opponent's",
                PlayerRefAst::You => "your",
                _ => return None,
            };
            Some(InstructionAst::ExileGraveyard {
                whose: mapped.to_owned(),
            })
        }
        _ => None,
    }
}

pub(super) fn push_damage_target(instructions: &mut Vec<Instruction>, amt: ValueExpr) {
    if let ValueExpr::Literal(n) = amt {
        instructions.push(Instruction::Damage { amount: n });
    } else {
        instructions.push(Instruction::DamageAmount(amt));
    }
}

pub(super) fn lower_damage_group(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<DamageGroup, &'static str> {
    if f.any_types == AstCardTypeMask::CREATURE
        && f.forbidden_types.is_empty()
        && f.controller.is_none()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
    {
        if f.forbidden_keywords == ["flying"] {
            return Ok(DamageGroup::CreaturesWithoutFlying);
        }
        if f.subtypes.len() == 1
            && let Some(ct) = vocabulary.resolve_label(&f.subtypes[0])
        {
            return Ok(DamageGroup::CreaturesOfSubtype(ct));
        }
        if f.forbidden_subtypes.len() == 1
            && let Some(ct) = vocabulary.resolve_label(&f.forbidden_subtypes[0])
        {
            return Ok(DamageGroup::CreaturesExcept(ct));
        }
        if f.subtypes.is_empty()
            && f.forbidden_subtypes.is_empty()
            && f.required_keywords.is_empty()
            && f.forbidden_keywords.is_empty()
            && f.flags.is_empty()
        {
            return Ok(DamageGroup::Creatures);
        }
    }
    if f.any_types == AstCardTypeMask::CREATURE.union(AstCardTypeMask::PLANESWALKER)
        && f.forbidden_types.is_empty()
        && f.controller.is_none()
        && f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.flags.is_empty()
    {
        return Ok(DamageGroup::CreaturesAndPlaneswalkers);
    }
    if f.zone.is_none() {
        let obj = lower_object_filter(f, vocabulary)?;
        return Ok(DamageGroup::Filtered(obj));
    }
    Err("oracle:unsupported-damage-group")
}

pub(super) fn scale_value_expr(expr: ValueExpr, factor: i64) -> ValueExpr {
    match factor {
        0 => ValueExpr::literal(0),
        1 => expr,
        -1 => ValueExpr::Sub(Box::new(ValueExpr::literal(0)), Box::new(expr)),
        f => ValueExpr::Scale {
            expr: Box::new(expr),
            factor: f.into(),
        },
    }
}

pub(super) fn parse_dynamic_pt_expr(
    raw: &str,
    bound_x: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<ValueExpr> {
    let trimmed = raw.trim();
    if let Some(n) = parse_signed_pt_delta(trimmed) {
        return Some(ValueExpr::literal(n));
    }
    let low = trimmed.to_ascii_lowercase();
    let x_val = || -> Option<ValueExpr> {
        match bound_x {
            Some(bx) => lower_value_expr(bx, vocabulary).ok(),
            None => Some(ValueExpr::chosen_x()),
        }
    };
    if low == "x" || low == "+x" {
        return x_val();
    }
    if low == "-x" {
        return Some(scale_value_expr(x_val()?, -1));
    }
    if let Some(coeff) = low.strip_suffix('x')
        && let Ok(k) = coeff.parse::<i64>()
    {
        return Some(scale_value_expr(x_val()?, k));
    }
    None
}

pub(super) fn lower_modify_pt_part(
    subject: &SubjectRefAst,
    part: ResolvedPartSpec,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        s if is_self_pronoun(s, builder, self_pronoun_valid) => {
            builder.instructions.push(Instruction::ChangeAbilitySource {
                parts: ResolvedPartsSpec(vec![part]),
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
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::Exact(n),
        } if filter.zone.is_none() && *n >= 2 => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: *n,
                    maximum: *n,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::UpTo(max),
        } if filter.zone.is_none() && *max > 0 => {
            let bt = lower_battlefield_body_target(filter, vocabulary)?;
            builder.bind_target(bt)?;
            builder
                .instructions
                .push(Instruction::AffectPermanentTargets {
                    minimum: 0,
                    maximum: *max,
                    action: PermanentTargetAction::Modify {
                        parts: ResolvedPartsSpec(vec![part]),
                        duration: dur,
                    },
                });
            Ok(())
        }
        SubjectRefAst::TargetObject {
            filter,
            count: TargetCountAst::AnyNumber,
        } if filter.zone.is_none() => {
            let obj = lower_object_filter(filter, vocabulary)?;
            builder.bind_target(BodyTarget::FilteredPermanent(obj))?;
            builder
                .instructions
                .push(Instruction::ModifyAnyNumberOfPermanents {
                    parts: ResolvedPartsSpec(vec![part]),
                    duration: dur,
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_) if builder.target != BodyTarget::None => {
            builder
                .instructions
                .push(Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![part]),
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
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if in_trigger && !self_pronoun_valid && builder.target == BodyTarget::None =>
        {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::TriggerSubject),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) => {
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                    ObjectClause::any().attached_to_self(),
                ))),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() =>
        {
            let obj = lower_object_filter(f, vocabulary)?;
            builder.last_group_filter = Some(obj);
            builder.instructions.push(Instruction::ChangeGroup {
                group: Box::new(EffectSubject::Matching(obj)),
                parts: ResolvedPartsSpec(vec![part]),
                duration: dur,
            });
            Ok(())
        }
        _ => Err("oracle:unsupported-modify-pt-subject"),
    }
}

pub(super) fn lower_modify_pt_target(
    subject: &SubjectRefAst,
    pt: PowerToughness,
    dur: ResolvedDuration,
    builder: &mut BodyBuilder,
    vocabulary: &CreatureTypeVocabulary,
    in_trigger: bool,
    self_pronoun_valid: bool,
) -> Result<(), &'static str> {
    match subject {
        s if is_self_pronoun(s, builder, self_pronoun_valid)
            && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            if in_trigger {
                builder
                    .instructions
                    .push(Instruction::ModifyGroupUntilEndOfTurn {
                        group: Box::new(EffectSubject::AbilitySource),
                        power_toughness: Some(pt),
                        keywords: BTreeSet::new(),
                    });
            } else {
                builder.instructions.push(Instruction::BoostSource {
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            }
            Ok(())
        }
        SubjectRefAst::TriggerSubject if in_trigger && dur == ResolvedDuration::UntilEndOfTurn => {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::TriggerSubject),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if in_trigger
                && !self_pronoun_valid
                && builder.target == BodyTarget::None
                && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::TriggerSubject),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::Anaphora(_)
            if builder.target == BodyTarget::None
                && dur == ResolvedDuration::UntilEndOfTurn
                && let Some(group_obj) = builder.last_group_filter =>
        {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(group_obj)),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::AttachedHost(_) if dur == ResolvedDuration::UntilEndOfTurn => {
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(ObjectFilter::Clause(
                        ObjectClause::any().attached_to_self(),
                    ))),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        SubjectRefAst::All(f) | SubjectRefAst::Each(f) | SubjectRefAst::Group(f)
            if f.zone.is_none() && dur == ResolvedDuration::UntilEndOfTurn =>
        {
            let obj = lower_object_filter(f, vocabulary)?;
            builder.last_group_filter = Some(obj);
            builder
                .instructions
                .push(Instruction::ModifyGroupUntilEndOfTurn {
                    group: Box::new(EffectSubject::Matching(obj)),
                    power_toughness: Some(pt),
                    keywords: BTreeSet::new(),
                });
            Ok(())
        }
        _ => lower_modify_pt_part(
            subject,
            ResolvedPartSpec::ModifyPT(pt),
            dur,
            builder,
            vocabulary,
            in_trigger,
            self_pronoun_valid,
        ),
    }
}

pub(super) fn lower_sacrifice_selection(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<spellwake_engine::program::SacrificeSelection> {
    use spellwake_engine::program::SacrificeSelection;
    if !f.forbidden_types.is_empty()
        || !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !matches!(f.controller, None | Some(ControllerRelation::You))
        || f.zone.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.any_card
        || f.any_spell
        || f.is_ability
    {
        return None;
    }
    let exclude_self = f.flags.0 & AstFilterFlags::EXCLUDE_SELF.0 != 0;
    if f.flags.0 & !AstFilterFlags::EXCLUDE_SELF.0 != 0 {
        return None;
    }
    if f.subtypes.len() == 1 {
        if exclude_self
            || f.token.is_some()
            || (!f.any_types.is_empty() && f.any_types != AstCardTypeMask::CREATURE)
        {
            return None;
        }
        let ct = vocabulary.resolve_label(&f.subtypes[0])?;
        return Some(SacrificeSelection::Subtype(ct));
    }
    if !f.subtypes.is_empty() {
        return None;
    }
    let nontoken = match f.token {
        None => false,
        Some(false) => true,
        Some(true) => return None,
    };
    let art_or_cre = AstCardTypeMask::ARTIFACT.union(AstCardTypeMask::CREATURE);
    if nontoken {
        if !exclude_self && f.any_types == AstCardTypeMask::CREATURE {
            return Some(SacrificeSelection::NontokenCreature);
        }
        return None;
    }
    if f.any_types == AstCardTypeMask::CREATURE {
        Some(if exclude_self {
            SacrificeSelection::OtherCreature
        } else {
            SacrificeSelection::Creature
        })
    } else if f.any_types == AstCardTypeMask::ARTIFACT {
        Some(if exclude_self {
            SacrificeSelection::OtherArtifact
        } else {
            SacrificeSelection::Artifact
        })
    } else if f.any_types == AstCardTypeMask::ENCHANTMENT && !exclude_self {
        Some(SacrificeSelection::Enchantment)
    } else if f.any_types == AstCardTypeMask::LAND && !exclude_self {
        Some(SacrificeSelection::Land)
    } else if f.any_types == art_or_cre {
        Some(if exclude_self {
            SacrificeSelection::OtherArtifactOrCreature
        } else {
            SacrificeSelection::ArtifactOrCreature
        })
    } else if f.any_types.is_empty() && f.any_permanent {
        Some(if exclude_self {
            SacrificeSelection::OtherPermanent
        } else {
            SacrificeSelection::Permanent
        })
    } else {
        None
    }
}

pub(super) fn try_build_offer_mana(
    payer: ResolvingPlayer,
    raw: &str,
    paid_destination: usize,
    declined_destination: usize,
    bound_x: Option<&ValueExprAst>,
    vocabulary: &CreatureTypeVocabulary,
) -> Option<Instruction> {
    if let Some(cost) = parse_fixed_mana_cost(raw) {
        return Some(Instruction::OfferManaPayment {
            payer,
            cost,
            paid_destination,
            declined_destination,
        });
    }
    if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
        && x_cnt > 0
    {
        let x_val = match bound_x {
            Some(expr) => lower_value_expr(expr, vocabulary).ok()?,
            None => ValueExpr::chosen_x(),
        };
        let extra_generic = scale_value_expr(x_val, i64::from(x_cnt));
        return Some(Instruction::OfferDynamicManaPayment {
            payer,
            fixed,
            extra_generic,
            paid_destination,
            declined_destination,
        });
    }
    None
}

pub(super) fn with_offer_dests(mut op: Instruction, paid: usize, declined: usize) -> Instruction {
    match &mut op {
        Instruction::OfferManaPayment {
            paid_destination,
            declined_destination,
            ..
        }
        | Instruction::OfferDynamicManaPayment {
            paid_destination,
            declined_destination,
            ..
        } => {
            *paid_destination = paid;
            *declined_destination = declined;
        }
        _ => {}
    }
    op
}

pub(super) fn set_offer_paid(op: Option<&mut Instruction>, paid: usize) {
    if let Some(
        Instruction::OfferManaPayment {
            paid_destination, ..
        }
        | Instruction::OfferDynamicManaPayment {
            paid_destination, ..
        },
    ) = op
    {
        *paid_destination = paid;
    }
}

pub(super) fn set_offer_declined(op: Option<&mut Instruction>, declined: usize) {
    if let Some(
        Instruction::OfferManaPayment {
            declined_destination,
            ..
        }
        | Instruction::OfferDynamicManaPayment {
            declined_destination,
            ..
        },
    ) = op
    {
        *declined_destination = declined;
    }
}

pub(super) fn lower_resolution_pay_cost_ops(
    player: ResolvingPlayer,
    cost: &CostAtomAst,
    vocabulary: &CreatureTypeVocabulary,
    bound_x: Option<&ValueExprAst>,
) -> Result<Vec<Instruction>, &'static str> {
    let lower_filter_any = |f: &ObjectFilterAst| -> Result<ObjectFilter, &'static str> {
        let mut inner = f.clone();
        inner.zone = None;
        if inner.is_empty() || inner.any_card {
            Ok(ObjectFilter::any())
        } else {
            lower_object_filter(&inner, vocabulary)
        }
    };
    match cost {
        CostAtomAst::PayLife(expr) => {
            let amt = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?;
            if player == ResolvingPlayer::Controller
                && let ValueExpr::Literal(n) = &amt
                && let Some(u) = n.to_usize().and_then(|v| u16::try_from(v).ok())
                && u > 0
            {
                return Ok(vec![Instruction::PayControllerLife(u)]);
            }
            Ok(vec![Instruction::PayLife {
                player,
                amount: amt,
            }])
        }
        CostAtomAst::PayEnergy(n) if *n > 0 => {
            if player == ResolvingPlayer::Controller {
                Ok(vec![Instruction::PayControllerEnergy(*n)])
            } else {
                Ok(vec![Instruction::PayEnergy {
                    player,
                    amount: ValueExpr::literal(i64::from(*n)),
                }])
            }
        }
        CostAtomAst::PayEnergyExpr(expr) => {
            let amt = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?;
            Ok(vec![Instruction::PayEnergy {
                player,
                amount: amt,
            }])
        }
        CostAtomAst::SacrificeSelf => Ok(vec![Instruction::SacrificeStateTriggerSource]),
        CostAtomAst::ExileSelf { .. } => Ok(vec![Instruction::ExileAbilitySource]),
        CostAtomAst::ExertSelf => Ok(vec![Instruction::ExertAbilitySource]),
        CostAtomAst::Behold(filter) => {
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::Behold {
                filter: Box::new(obj),
                optional: false,
            }])
        }
        CostAtomAst::RollDie(sides) => {
            let nz = std::num::NonZeroU16::new(u16::from(*sides)).ok_or("oracle:zero-die-sides")?;
            Ok(vec![Instruction::RollDie {
                player,
                sides: nz,
                count: ValueExpr::literal(1),
                mode: spellwake_engine::program::DieRollMode::Sum,
                bands: Vec::new(),
            }])
        }
        CostAtomAst::DiscardHand => Ok(vec![Instruction::Discard {
            players: AffectedPlayers::One(player),
            amount: ValueExpr::count_hand(player),
        }]),
        CostAtomAst::Discard {
            count,
            filter,
            random,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let plain_any = filter.is_empty() || filter.any_card;
            if *random {
                if plain_any && amt == ValueExpr::literal(1) {
                    return Ok(vec![Instruction::DiscardOneAtRandom(player)]);
                }
                return Err("oracle:unsupported-pay-cost");
            }
            if plain_any {
                Ok(vec![Instruction::Discard {
                    players: AffectedPlayers::One(player),
                    amount: amt,
                }])
            } else {
                let obj = lower_object_filter(filter, vocabulary)?;
                Ok(vec![Instruction::DiscardFiltered {
                    players: AffectedPlayers::One(player),
                    amount: amt,
                    filter: Box::new(obj),
                }])
            }
        }
        CostAtomAst::Sacrifice { count, filter } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let ptf = lower_permanent_type_filter(filter, vocabulary)?;
            Ok(vec![Instruction::Sacrifice {
                players: AffectedPlayers::One(player),
                amount: amt,
                filter: ptf,
            }])
        }
        CostAtomAst::Exile { count, filter } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let zone = match filter.zone {
                Some(ZoneFilter::YourGraveyard) => CostExileZone::Graveyard,
                Some(ZoneFilter::YourHand) => CostExileZone::Hand,
                None | Some(ZoneFilter::Battlefield) => CostExileZone::Battlefield,
                _ => return Err("oracle:unsupported-pay-cost"),
            };
            if zone == CostExileZone::Graveyard
                && (filter.any_card || filter.any_types.is_empty())
                && filter.forbidden_types.is_empty()
                && filter.subtypes.is_empty()
                && filter.any_colors.is_empty()
                && filter.forbidden_colors.is_empty()
                && filter.stat_bounds.is_empty()
                && let ValueExpr::Literal(n) = &amt
                && let Some(nz) = n
                    .to_usize()
                    .and_then(|v| u8::try_from(v).ok())
                    .and_then(NonZeroU8::new)
            {
                return Ok(vec![Instruction::ExileCardsFromGraveyard {
                    player,
                    count: nz,
                }]);
            }
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::ExileFromZone {
                players: AffectedPlayers::One(player),
                zone,
                amount: amt,
                filter: Box::new(obj),
            }])
        }
        CostAtomAst::BounceToHand(subject) => match subject.as_ref() {
            SubjectRefAst::SelfObject => Ok(vec![Instruction::ReturnSourceToHand]),
            SubjectRefAst::Group(f) | SubjectRefAst::All(f) | SubjectRefAst::Each(f) => {
                let ptf = lower_permanent_type_filter(f, vocabulary)?;
                Ok(vec![Instruction::ReturnControlledPermanents {
                    players: AffectedPlayers::One(player),
                    amount: ValueExpr::literal(1),
                    filter: ptf,
                }])
            }
            _ => Err("oracle:unsupported-pay-cost"),
        },
        CostAtomAst::TapPermanent { count, filter } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::TapOrUntapControlled {
                players: AffectedPlayers::One(player),
                tap: true,
                amount: amt,
                filter: Box::new(obj),
            }])
        }
        CostAtomAst::UntapPermanent { count, filter } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let obj = lower_filter_any(filter)?;
            Ok(vec![Instruction::TapOrUntapControlled {
                players: AffectedPlayers::One(player),
                tap: false,
                amount: amt,
                filter: Box::new(obj),
            }])
        }
        CostAtomAst::Mill(expr) => {
            let amt = lower_value_expr(resolve_x(expr, bound_x), vocabulary)?;
            Ok(vec![Instruction::Mill {
                player,
                amount: amt,
            }])
        }
        CostAtomAst::CollectEvidence(_) | CostAtomAst::Blight(_) => {
            Err("oracle:unsupported-pay-cost")
        }
        _ => Err("oracle:unsupported-pay-cost"),
    }
}

pub(super) fn lower_ward_cost_body(
    atoms: &[CostAtomAst],
    vocabulary: &CreatureTypeVocabulary,
) -> Result<ExecutableBody, &'static str> {
    let payer = ResolvingPlayer::UniqueRelatedPlayer;
    if let [CostAtomAst::PayMana(raw)] = atoms {
        if let Some((fixed, x_cnt)) = parse_mana_cost_with_x(raw)
            && x_cnt > 0
        {
            let extra_generic = scale_value_expr(ValueExpr::chosen_x(), i64::from(x_cnt));
            return Ok(ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::OfferDynamicManaPayment {
                    payer,
                    fixed,
                    extra_generic,
                    paid_destination: 2,
                    declined_destination: 1,
                }],
            ));
        }
        return Err("oracle:ward-non-mana");
    }
    let mut mana_cost: Option<ManaCost> = None;
    let mut non_mana_ops: Vec<Instruction> = Vec::new();
    for atom in atoms {
        if let CostAtomAst::PayMana(raw) = atom {
            let mc = parse_fixed_mana_cost(raw).ok_or("oracle:ward-non-mana")?;
            let entry = mana_cost.get_or_insert_with(ManaCost::free);
            entry.generic = entry.generic.plus(&mc.generic);
            for (k, v) in mc.required {
                let slot = entry.required.entry(k).or_insert_with(|| 0.into());
                *slot = slot.plus(&v);
            }
        } else {
            let ops = lower_resolution_pay_cost_ops(payer, atom, vocabulary, None)
                .map_err(|_| "oracle:ward-non-mana")?;
            non_mana_ops.extend(ops);
        }
    }
    let mut instructions = Vec::new();
    if let Some(mc) = mana_cost {
        if non_mana_ops.is_empty() {
            instructions.push(Instruction::OfferManaPayment {
                payer,
                cost: mc,
                paid_destination: 2,
                declined_destination: 1,
            });
            return Ok(ExecutableBody::new(BodyTarget::None, instructions));
        }
        // Offer mana first; if declined jump directly to CounterWardSource (the instruction after this body).
        let counter_idx = 1 + 1 + non_mana_ops.len() + 1;
        instructions.push(Instruction::OfferManaPayment {
            payer,
            cost: mc,
            paid_destination: 1,
            declined_destination: counter_idx,
        });
    }
    if non_mana_ops.is_empty() {
        return Err("oracle:ward-non-mana");
    }
    if non_mana_ops.len() == 1 {
        instructions.push(Instruction::OptionalEffect { chooser: payer });
    } else {
        instructions.push(Instruction::OptionalSequence {
            chooser: payer,
            instructions: non_mana_ops.len(),
        });
    }
    instructions.extend(non_mana_ops);
    let branch_idx = instructions.len();
    instructions.push(Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left: ValueExpr::event(EventValueKind::LastActionSucceeded),
            comparison: Comparison::Less,
            right: ValueExpr::literal(1),
        },
        // Skip CounterWardSource (which caller appends at branch_idx + 1) when paid.
        destination: branch_idx + 2,
    });
    Ok(ExecutableBody::new(BodyTarget::None, instructions))
}

pub(super) fn lower_permanent_type_filter(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<PermanentTypeFilter, &'static str> {
    if f.subtypes.is_empty()
        && f.forbidden_subtypes.is_empty()
        && f.any_colors.is_empty()
        && f.forbidden_colors.is_empty()
        && f.color_arity.is_none()
        && f.tapped.is_none()
        && f.token.is_none()
        && f.stat_bounds.is_empty()
        && f.required_supertypes.is_empty()
        && f.forbidden_supertypes.is_empty()
        && f.required_keywords.is_empty()
        && f.forbidden_keywords.is_empty()
        && f.counter_kind.is_none()
        && f.flags.0 == 0
    {
        return Ok(PermanentTypeFilter {
            any_types: card_type_mask_to_set(f.any_types)?,
            required_types: BTreeSet::new(),
            forbidden_types: card_type_mask_to_set(f.forbidden_types)?,
            object_filter: None,
        });
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(PermanentTypeFilter {
        any_types: BTreeSet::new(),
        required_types: BTreeSet::new(),
        forbidden_types: BTreeSet::new(),
        object_filter: Some(obj),
    })
}

// Internal lowering helper taking the orthogonal fields of `InstructionAst::SearchLibrary` plus lowering flags.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_library_search(
    library: &str,
    count: &ValueExprAst,
    filter: &ObjectFilterAst,
    reveal: bool,
    to_zone: &str,
    vocabulary: &CreatureTypeVocabulary,
    optional_search: bool,
    up_to: bool,
    any_number: bool,
) -> Result<LibrarySearch, &'static str> {
    let lib_low = library.trim().to_ascii_lowercase();
    let (searcher, source) = match lib_low.as_str() {
        "your" | "own" | "" => (ResolvingPlayer::Controller, SearchSource::OwnLibrary),
        "your library and/or graveyard"
        | "your graveyard and/or library"
        | "your library and graveyard"
        | "your library, hand, and/or graveyard"
        | "your graveyard and library" => (
            ResolvingPlayer::Controller,
            SearchSource::OwnLibraryOrGraveyard,
        ),
        "target opponent" | "target opponent's" | "target opponent's library" => (
            ResolvingPlayer::Controller,
            SearchSource::TargetOpponentLibrary,
        ),
        "that player" | "that player's" | "that player's library" | "their" | "their library" => (
            ResolvingPlayer::UniqueRelatedPlayer,
            SearchSource::OwnLibrary,
        ),
        "target player" | "target player's" | "target player's library" => {
            (ResolvingPlayer::TargetPlayer, SearchSource::OwnLibrary)
        }
        _ => return Err("oracle:unsupported-search-source"),
    };
    let search_count = if any_number {
        SearchCount::UpTo(u8::MAX)
    } else {
        let ValueExprAst::Const(n) = count else {
            return Err("oracle:dynamic-search-count");
        };
        let n_u8 = u8::try_from(*n).map_err(|_| "oracle:search-count-ovf")?;
        if n_u8 == 0 {
            return Err("oracle:search-zero");
        }
        let matcher_preview = lower_search_matcher(filter, vocabulary)?;
        if matcher_preview == SearchCardMatcher::Any && !optional_search && !up_to {
            SearchCount::Exact(NonZeroU8::new(n_u8).ok_or("oracle:search-zero")?)
        } else {
            SearchCount::UpTo(n_u8)
        }
    };
    let matcher = lower_search_matcher(filter, vocabulary)?;
    let dest_low = to_zone.to_ascii_lowercase();
    let destination = if dest_low.contains("hand") {
        SearchDestination::Hand
    } else if dest_low.contains("graveyard") {
        SearchDestination::Graveyard
    } else if dest_low.contains("battlefield") {
        SearchDestination::Battlefield {
            tapped: dest_low.contains("tapped"),
            post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
        }
    } else if dest_low.contains("top") {
        SearchDestination::LibraryTop
    } else if dest_low.contains("exile") {
        SearchDestination::Exile {
            link: SearchExileLink::None,
        }
    } else {
        return Err("oracle:unsupported-search-dest");
    };
    let mut ls = LibrarySearch {
        searcher,
        source,
        matcher,
        mana_value: None,
        count: search_count,
        pair_constraint: None,
        destination,
        reveal,
    };
    if !matches!(ls.matcher, SearchCardMatcher::Filtered(_)) {
        if filter.stat_bounds.len() == 1 {
            let sb = &filter.stat_bounds[0];
            if sb.stat == StatKind::ManaValue {
                ls.mana_value = match (sb.op, sb.value.as_ref()) {
                    (CompareOp::Le, ValueExprAst::Const(v)) if *v >= 0 => Some(
                        SearchManaValue::AtMost(u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?),
                    ),
                    (CompareOp::Ge, ValueExprAst::Const(v)) if *v >= 0 => Some(
                        SearchManaValue::AtLeast(u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?),
                    ),
                    (CompareOp::Eq, ValueExprAst::Const(v)) if *v >= 0 => {
                        Some(SearchManaValue::Exact(i64::from(*v).into()))
                    }
                    (CompareOp::Le, ValueExprAst::ChosenX) => {
                        Some(SearchManaValue::AtMostAnnouncedX)
                    }
                    _ => return Err("oracle:unsupported-search-mv"),
                };
            } else {
                let obj = lower_object_filter(filter, vocabulary)?;
                ls.matcher = SearchCardMatcher::Filtered(Box::new(obj));
            }
        } else if !filter.stat_bounds.is_empty() {
            let obj = lower_object_filter(filter, vocabulary)?;
            ls.matcher = SearchCardMatcher::Filtered(Box::new(obj));
        }
    }
    Ok(ls)
}

pub(super) fn lower_search_matcher(
    f: &ObjectFilterAst,
    vocabulary: &CreatureTypeVocabulary,
) -> Result<SearchCardMatcher, &'static str> {
    if let Some(named) = &f.named {
        use spellwake_oracle_compiler::ast::NameFilterAst;
        return match named {
            NameFilterAst::Exact(n) => {
                let card_name = EnglishCardName::new(n.clone())
                    .map_err(|_| "oracle:unsupported-search-named")?;
                Ok(SearchCardMatcher::AnyOfNames(vec![card_name]))
            }
            _ => Err("oracle:unsupported-search-named"),
        };
    }
    let has_complex = !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || !f.forbidden_supertypes.is_empty()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.flags.is_empty();
    if !has_complex {
        if f.any_card
            && f.any_types.is_empty()
            && f.forbidden_types.is_empty()
            && f.subtypes.is_empty()
            && f.required_supertypes.is_empty()
        {
            return Ok(SearchCardMatcher::Any);
        }
        if f.forbidden_types == AstCardTypeMask::LAND
            && f.any_types.is_empty()
            && f.subtypes.is_empty()
            && f.required_supertypes.is_empty()
        {
            return Ok(SearchCardMatcher::Nonland);
        }
        if f.required_supertypes == AstSupertypeMask::BASIC
            && f.any_types == AstCardTypeMask::LAND
            && f.subtypes.is_empty()
            && f.forbidden_types.is_empty()
        {
            return Ok(SearchCardMatcher::BasicLand);
        }
        if f.required_supertypes.is_empty() && f.forbidden_types.is_empty() {
            if !f.subtypes.is_empty() {
                if f.subtypes.len() == 1 {
                    let st = &f.subtypes[0];
                    if st.eq_ignore_ascii_case("equipment") {
                        return Ok(SearchCardMatcher::Equipment);
                    }
                    if let Some(ct) = vocabulary.resolve_label(st) {
                        return Ok(SearchCardMatcher::CreatureSubtype(ct));
                    }
                }
                let mut lands = BTreeSet::new();
                let mut all_lands = true;
                for st in &f.subtypes {
                    if let Some(lt) = parse_land_subtype(st) {
                        lands.insert(lt);
                    } else {
                        all_lands = false;
                        break;
                    }
                }
                if all_lands && !lands.is_empty() {
                    return Ok(SearchCardMatcher::LandSubtypes {
                        subtypes: lands,
                        require_land_type: f.any_types.0 & AstCardTypeMask::LAND.0 != 0,
                    });
                }
            } else {
                let types = card_type_mask_to_set(f.any_types)?;
                if !types.is_empty() {
                    return Ok(SearchCardMatcher::CardTypes(types));
                }
                return Ok(SearchCardMatcher::Any);
            }
        }
    }
    let obj = lower_object_filter(f, vocabulary)?;
    Ok(SearchCardMatcher::Filtered(Box::new(obj)))
}

fn normalize_token_ability_text(raw: &str, tok_name: &str) -> String {
    let mut s = raw
        .trim()
        .replace("this token", "~")
        .replace("This token", "~")
        .replace("this creature", "~")
        .replace("This creature", "~")
        .replace("this artifact", "~")
        .replace("This artifact", "~");
    let tok_low = tok_name.trim().to_ascii_lowercase();
    if !tok_low.is_empty() && tok_low != "token" {
        // Replace case-insensitive token name occurrences with "~"
        let s_low = s.to_ascii_lowercase();
        let mut out = String::with_capacity(s.len());
        let mut cursor = 0usize;
        while let Some(rel) = s_low[cursor..].find(&tok_low) {
            let start = cursor + rel;
            let end = start + tok_low.len();
            let before_ok = start == 0 || !s.as_bytes()[start - 1].is_ascii_alphanumeric();
            let after_ok = end == s.len() || !s.as_bytes()[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                out.push_str(&s[cursor..start]);
                out.push('~');
            } else {
                out.push_str(&s[cursor..end]);
            }
            cursor = end;
        }
        out.push_str(&s[cursor..]);
        s = out;
    }
    // Uppercase mana/tap/untap symbols inside braces (`{t}` -> `{T}`, `{w}` -> `{W}`, `{w/u}` -> `{W/U}`, etc.)
    let mut brace_norm = String::with_capacity(s.len());
    let mut in_brace = false;
    for ch in s.chars() {
        if ch == '{' {
            in_brace = true;
            brace_norm.push(ch);
        } else if ch == '}' {
            in_brace = false;
            brace_norm.push(ch);
        } else if in_brace {
            brace_norm.push(ch.to_ascii_uppercase());
        } else {
            brace_norm.push(ch);
        }
    }
    // Sentence-case the start and after `. ` or `: ` so `oracle_compiler`'s capitalized clause headers match.
    let mut result = String::with_capacity(brace_norm.len());
    let mut cap_next = true;
    let mut prev_punct = false;
    for ch in brace_norm.chars() {
        if cap_next && ch.is_ascii_alphabetic() {
            result.push(ch.to_ascii_uppercase());
            cap_next = false;
            prev_punct = false;
        } else {
            result.push(ch);
            if ch == '.' || ch == ':' || ch == '\n' {
                prev_punct = true;
                if ch == '\n' {
                    cap_next = true;
                }
            } else if prev_punct && ch == ' ' {
                cap_next = true;
                prev_punct = false;
            } else if !ch.is_ascii_whitespace() {
                prev_punct = false;
                if ch == '~' || ch == '{' {
                    cap_next = false;
                }
            }
        }
    }
    result
}

pub(super) fn lower_token_spec(
    spec: &TokenSpecAst,
    recipients: TokenRecipients,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
    bound_x: Option<&ValueExprAst>,
) -> Result<(TokenCreation, bool), &'static str> {
    match spec {
        TokenSpecAst::Predefined {
            name,
            count,
            tapped,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let def = predefined_token_def(name, vocabulary, ids)?;
            Ok((
                TokenCreation {
                    enters_tapped: *tapped,
                    definition: def,
                    amount: amt,
                    base_size: None,
                    recipients,
                },
                false,
            ))
        }
        TokenSpecAst::Creature {
            count,
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let (p_val, t_val, base_size) =
                if power.eq_ignore_ascii_case("x") && toughness.eq_ignore_ascii_case("x") {
                    let x_expr = if let Some(bx) = bound_x {
                        lower_value_expr(bx, vocabulary)?
                    } else {
                        ValueExpr::chosen_x()
                    };
                    (0i64, 0i64, Some(TokenSize::AtCreation(x_expr)))
                } else {
                    let p: i64 = power.parse().map_err(|_| "oracle:token-pt")?;
                    let t: i64 = toughness.parse().map_err(|_| "oracle:token-pt")?;
                    (p, t, None)
                };

            let mut creature_subtypes = BTreeSet::new();
            let mut artifact_subtypes = BTreeSet::new();
            let mut enchantment_subtypes = BTreeSet::new();
            let mut land_subtypes = BTreeSet::new();
            let mut ordered_subtypes = Vec::new();
            for st in subtypes {
                if let Some(ct) = vocabulary.resolve_label(st) {
                    if creature_subtypes.insert(ct) {
                        ordered_subtypes.push(TokenSubtype::Creature(ct));
                    }
                } else if let Some(at) = ArtifactSubtype::from_wire_label(st) {
                    if artifact_subtypes.insert(at) {
                        ordered_subtypes.push(TokenSubtype::Artifact(at));
                    }
                } else if let Some(et) = EnchantmentSubtype::from_wire_label(st) {
                    if enchantment_subtypes.insert(et) {
                        ordered_subtypes.push(TokenSubtype::Enchantment(et));
                    }
                } else if let Some(lt) = LandType::from_wire_label(st) {
                    land_subtypes.insert(lt);
                } else {
                    return Err("oracle:token-unknown-subtype");
                }
            }

            let mut p = Program::creature(None, p_val.into(), t_val.into(), creature_subtypes);
            let extra_types = card_type_mask_to_set(*card_types)?;
            for t in extra_types {
                if t.is_permanent() {
                    p.card_types.insert(t);
                }
            }
            p.supertypes = supertype_mask_to_set(*supertypes)?;
            p.artifact_subtypes = artifact_subtypes;
            p.enchantment_subtypes = enchantment_subtypes;
            p.land_subtypes = land_subtypes;
            p.effect_defined_colors = Some(color_mask_to_set(*colors));

            let mut tok_ids = LoweringIds::default();
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            for kw_raw in keywords {
                if kw_raw.contains('"') {
                    let mut rest = kw_raw.as_str();
                    let mut quoted_texts: Vec<String> = Vec::new();
                    let mut leading_text = String::new();
                    while let Some(open) = rest.find('"') {
                        leading_text.push_str(&rest[..open]);
                        let after_open = &rest[open + 1..];
                        if let Some(close) = after_open.find('"') {
                            quoted_texts.push(after_open[..close].trim().to_string());
                            rest = &after_open[close + 1..];
                        } else {
                            return Err("oracle:unsupported-granted-keyword");
                        }
                    }
                    leading_text.push_str(rest);
                    for part in leading_text
                        .split([',', ';'])
                        .flat_map(|s| s.split(" and "))
                    {
                        let trimmed = part.trim().trim_start_matches("with ").trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Some(kw) = lower_keyword_ability_name(trimmed) {
                            p.printed_keywords.insert(kw);
                        } else {
                            let parsed_kw = compile_card(
                                &CardInput {
                                    oracle_id: "",
                                    name: "Token",
                                    layout: "normal",
                                    faces: vec![FaceInput {
                                        name: "Token",
                                        type_line: "Creature",
                                        oracle_text: trimmed,
                                    }],
                                },
                                &compiler_vocab,
                            );
                            if !parsed_kw.strictly_compiled {
                                return Err("oracle:unsupported-granted-keyword");
                            }
                            if let Some(face_ast) = parsed_kw.faces.first() {
                                for (k_idx, kw_ast) in face_ast.keywords.iter().enumerate() {
                                    lower_keyword(
                                        kw_ast,
                                        k_idx,
                                        face_ast,
                                        &mut p,
                                        vocabulary,
                                        &mut tok_ids,
                                        false,
                                    )
                                    .map_err(|_| "oracle:unsupported-granted-keyword")?;
                                }
                                for st in &face_ast.statics {
                                    lower_static(st, &mut p, vocabulary, &mut tok_ids, false)
                                        .map_err(|_| "oracle:unsupported-granted-keyword")?;
                                }
                                for rep in &face_ast.replacements {
                                    lower_replacement(rep, &mut p, vocabulary, &mut tok_ids)
                                        .map_err(|_| "oracle:unsupported-granted-keyword")?;
                                }
                                for ab in &face_ast.abilities {
                                    lower_ability(ab, &mut p, vocabulary, &mut tok_ids)
                                        .map_err(|_| "oracle:unsupported-granted-keyword")?;
                                }
                            }
                        }
                    }
                    let tok_name = named.as_deref().unwrap_or("Token");
                    for q in quoted_texts {
                        if q.is_empty() {
                            continue;
                        }
                        let norm_q = normalize_token_ability_text(&q, tok_name);
                        let compiled_q = compile_card(
                            &CardInput {
                                oracle_id: "",
                                name: tok_name,
                                layout: "normal",
                                faces: vec![FaceInput {
                                    name: tok_name,
                                    type_line: "Creature",
                                    oracle_text: &norm_q,
                                }],
                            },
                            &compiler_vocab,
                        );
                        if !compiled_q.strictly_compiled {
                            return Err("oracle:unsupported-granted-keyword");
                        }
                        let Some(face_ast) = compiled_q.faces.first() else {
                            return Err("oracle:unsupported-granted-keyword");
                        };
                        if !face_ast.spell_effects.is_empty() {
                            return Err("oracle:unsupported-granted-keyword");
                        }
                        for (k_idx, kw_ast) in face_ast.keywords.iter().enumerate() {
                            lower_keyword(
                                kw_ast,
                                k_idx,
                                face_ast,
                                &mut p,
                                vocabulary,
                                &mut tok_ids,
                                false,
                            )
                            .map_err(|_| "oracle:unsupported-granted-keyword")?;
                        }
                        for st in &face_ast.statics {
                            lower_static(st, &mut p, vocabulary, &mut tok_ids, false)
                                .map_err(|_| "oracle:unsupported-granted-keyword")?;
                        }
                        for rep in &face_ast.replacements {
                            lower_replacement(rep, &mut p, vocabulary, &mut tok_ids)
                                .map_err(|_| "oracle:unsupported-granted-keyword")?;
                        }
                        for ab in &face_ast.abilities {
                            lower_ability(ab, &mut p, vocabulary, &mut tok_ids)
                                .map_err(|_| "oracle:unsupported-granted-keyword")?;
                        }
                    }
                } else if let Some(kw) = lower_keyword_ability_name(kw_raw) {
                    p.printed_keywords.insert(kw);
                } else {
                    let tok_name = named.as_deref().unwrap_or("Token");
                    let norm_kw = normalize_token_ability_text(kw_raw, tok_name);
                    let compiled_kw = compile_card(
                        &CardInput {
                            oracle_id: "",
                            name: tok_name,
                            layout: "normal",
                            faces: vec![FaceInput {
                                name: tok_name,
                                type_line: "Creature",
                                oracle_text: &norm_kw,
                            }],
                        },
                        &compiler_vocab,
                    );
                    if !compiled_kw.strictly_compiled {
                        return Err("oracle:unsupported-granted-keyword");
                    }
                    let Some(face_ast) = compiled_kw.faces.first() else {
                        return Err("oracle:unsupported-granted-keyword");
                    };
                    if !face_ast.spell_effects.is_empty()
                        || (face_ast.keywords.is_empty()
                            && face_ast.statics.is_empty()
                            && face_ast.replacements.is_empty()
                            && face_ast.abilities.is_empty())
                    {
                        return Err("oracle:unsupported-granted-keyword");
                    }
                    for (k_idx, kw_ast) in face_ast.keywords.iter().enumerate() {
                        lower_keyword(
                            kw_ast,
                            k_idx,
                            face_ast,
                            &mut p,
                            vocabulary,
                            &mut tok_ids,
                            false,
                        )
                        .map_err(|_| "oracle:unsupported-granted-keyword")?;
                    }
                    for st in &face_ast.statics {
                        lower_static(st, &mut p, vocabulary, &mut tok_ids, false)
                            .map_err(|_| "oracle:unsupported-granted-keyword")?;
                    }
                    for rep in &face_ast.replacements {
                        lower_replacement(rep, &mut p, vocabulary, &mut tok_ids)
                            .map_err(|_| "oracle:unsupported-granted-keyword")?;
                    }
                    for ab in &face_ast.abilities {
                        lower_ability(ab, &mut p, vocabulary, &mut tok_ids)
                            .map_err(|_| "oracle:unsupported-granted-keyword")?;
                    }
                }
            }

            let token_name = if let Some(n) = named {
                TokenName::Specified(
                    EnglishCardName::new(n.clone()).map_err(|_| "oracle:token-name")?,
                )
            } else {
                if ordered_subtypes.is_empty() {
                    return Err("oracle:nameless-token");
                }
                TokenName::Default { ordered_subtypes }
            };

            let def = TokenDefinition::new(p, token_name, vocabulary)
                .map_err(|_| "oracle:token-def-invalid")?;
            Ok((
                TokenCreation {
                    enters_tapped: *tapped,
                    definition: def,
                    amount: amt,
                    base_size,
                    recipients,
                },
                *attacking,
            ))
        }
        _ => Err("oracle:unsupported-token-spec"),
    }
}

pub(super) fn predefined_token_def(
    name: &str,
    vocabulary: &CreatureTypeVocabulary,
    ids: &mut LoweringIds,
) -> Result<TokenDefinition, &'static str> {
    match name.to_ascii_lowercase().as_str() {
        "treasure" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Treasure);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: true },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Treasure)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "clue" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::Sacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Clue);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Clue)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "food" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(3.into()),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Food);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Food)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "gold" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Gold);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: false },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Gold)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "blood" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::Discard {
                        mode: DiscardCostMode::SelectedOne,
                        tap_source: true,
                        sacrifice_source: true,
                    },
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Blood);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Blood)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "junk" => {
            let act_id = ids.activated();
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::ExileTopForPlay {
                            until_next_from_source: false,
                        }],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Junk);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Junk)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "shard" => {
            let mut p = Program::enchantment(None);
            p.activated.push(ActivatedAbility {
                id: ids.activated(),
                mana_cost: ManaCost::generic(2.into()),
                source_cost: SourceCost::Sacrifice,
                body: ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::Scry {
                            player: ResolvingPlayer::Controller,
                            amount: ValueExpr::literal(1),
                        },
                        Instruction::DrawController {
                            amount: ValueExpr::literal(1),
                        },
                    ],
                ),
            });
            p.enchantment_subtypes.insert(EnchantmentSubtype::Shard);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Enchantment(EnchantmentSubtype::Shard)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "walker" => {
            let zombie = vocabulary
                .resolve_label("Zombie")
                .ok_or("oracle:predef-token-invalid")?;
            let mut p = Program::creature(None, 2.into(), 2.into(), BTreeSet::from([zombie]));
            p.effect_defined_colors = Some(BTreeSet::from([Color::Black]));
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Walker".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "etherium cell" => {
            let mut p = Program::artifact(None, vec![]);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: ManaOutput::all_colors(),
                }
                .with_modifier(ManaOutputModifier::Sacrifice(
                    ManaSacrificeCost::Source { tap: true },
                )),
            });
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new("Etherium Cell".to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "powerstone" => {
            let mut p = Program::artifact(None, vec![]);
            p.artifact_subtypes.insert(ArtifactSubtype::Powerstone);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.mana.push(ManaAbility {
                id: ids.mana(),
                output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Colourless, 1.into())]))
                    .restricted(ManaRestriction::NotNonartifactSpell),
            });
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Powerstone)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "lander" => {
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::None,
                        vec![Instruction::SearchLibrary {
                            filter: Box::new(LibrarySearch {
                                searcher: ResolvingPlayer::Controller,
                                source: SearchSource::OwnLibrary,
                                matcher: SearchCardMatcher::BasicLand,
                                mana_value: None,
                                count: SearchCount::UpTo(1),
                                pair_constraint: None,
                                destination: SearchDestination::Battlefield {
                                    tapped: true,
                                    post_entry:
                                        spellwake_engine::program::SearchBattlefieldPostEntry::None,
                                },
                                reveal: false,
                            }),
                        }],
                    ),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Lander);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Lander)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "incubator" => {
            let phyrexian = vocabulary
                .resolve_label("Phyrexian")
                .ok_or("oracle:predef-token-invalid")?;
            let mut back = Program::creature(None, 0.into(), 0.into(), BTreeSet::from([phyrexian]));
            back.card_types.insert(CardType::Artifact);
            back.effect_defined_colors = Some(BTreeSet::new());
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ids.activated(),
                    mana_cost: ManaCost::generic(2.into()),
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(BodyTarget::None, vec![Instruction::TransformSource]),
                }],
            );
            p.artifact_subtypes.insert(ArtifactSubtype::Incubator);
            p.effect_defined_colors = Some(BTreeSet::new());
            p.transform_back = Some(Box::new(back));
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Incubator)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "map" => {
            let act_id = ids.activated();
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::FilteredPermanent(ObjectFilter::creature().controlled()),
                        vec![Instruction::ExploreSubject(MoveSubjectRef::TargetObject)],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Map);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Map)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "mutagen" => {
            let act_id = ids.activated();
            let mut p = Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: act_id,
                    mana_cost: ManaCost::generic(1.into()),
                    source_cost: SourceCost::TapSacrifice,
                    body: ExecutableBody::new(
                        BodyTarget::SingleCreature,
                        vec![Instruction::AffectPermanentTargets {
                            minimum: 1,
                            maximum: 1,
                            action: PermanentTargetAction::PutCounters {
                                kind: CounterKind::PlusOne,
                                amount: ValueExpr::literal(1),
                            },
                        }],
                    ),
                }],
            );
            p.activation_restrictions
                .insert(act_id, ActivationRestriction::Sorcery);
            p.artifact_subtypes.insert(ArtifactSubtype::Mutagen);
            p.effect_defined_colors = Some(BTreeSet::new());
            TokenDefinition::new(
                p,
                TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Artifact(ArtifactSubtype::Mutagen)],
                },
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        "cursed role" | "monster role" | "royal role" | "sorcerer role" | "virtuous role"
        | "wicked role" | "young hero role" => {
            let mut p = Program::aura(None, EnchantRestriction::Creature);
            p.enchantment_subtypes.insert(EnchantmentSubtype::Role);
            p.effect_defined_colors = Some(BTreeSet::new());
            let attached_sel = PermanentSelector {
                attached_to_source: true,
                ..PermanentSelector::default()
            };
            let plus_one_pair = NumericPair {
                power: NumericExpression::Literal(1.into()),
                toughness: NumericExpression::Literal(1.into()),
            };
            let role_name = match name.to_ascii_lowercase().as_str() {
                "cursed role" => {
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::SetPT(plus_one_pair)]),
                        },
                    )?;
                    "Cursed"
                }
                "monster role" => {
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![
                                StaticPart::ModifyPT(plus_one_pair),
                                StaticPart::Abilities(AbilityChange {
                                    granted_keywords: BTreeSet::from([KeywordAbility::Trample]),
                                    ..AbilityChange::default()
                                }),
                            ]),
                        },
                    )?;
                    "Monster"
                }
                "royal role" => {
                    let ward = TriggeredAbility::ward(ids.triggered(), ManaCost::generic(1.into()));
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![
                                StaticPart::ModifyPT(plus_one_pair),
                                StaticPart::GrantTrigger(Box::new(ward)),
                            ]),
                        },
                    )?;
                    "Royal"
                }
                "sorcerer role" => {
                    let scry_trig = TriggeredAbility {
                        id: ids.triggered(),
                        condition: TriggerSpec::self_attacks(),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::Scry {
                                player: ResolvingPlayer::Controller,
                                amount: ValueExpr::literal(1),
                            }],
                        ),
                    };
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![
                                StaticPart::ModifyPT(plus_one_pair),
                                StaticPart::GrantTrigger(Box::new(scry_trig)),
                            ]),
                        },
                    )?;
                    "Sorcerer"
                }
                "virtuous role" => {
                    let ench_sel = PermanentSelector {
                        required_types: BTreeSet::from([CardType::Enchantment]),
                        controller: ControllerSelector::SourceController,
                        ..PermanentSelector::default()
                    };
                    let count_expr = NumericExpression::count(ench_sel);
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::ModifyPT(NumericPair {
                                power: count_expr.clone(),
                                toughness: count_expr,
                            })]),
                        },
                    )?;
                    "Virtuous"
                }
                "wicked role" => {
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::ModifyPT(plus_one_pair)]),
                        },
                    )?;
                    p.triggered.push(TriggeredAbility {
                        id: ids.triggered(),
                        condition: TriggerSpec::new(TriggerEvent::Dies {
                            subject: ObjectFilter::self_object(),
                            mode: DiesTriggerMode::Permanent,
                        }),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::AdjustEachOpponentLife {
                                adjustment: LifeAdjustment::Lose(1.into()),
                                controller_adjustment: None,
                            }],
                        ),
                    });
                    "Wicked"
                }
                "young hero role" => {
                    let attack_trig = TriggeredAbility {
                        id: ids.triggered(),
                        condition: TriggerSpec::self_attacks().only_if(TriggerCondition::Eval(
                            Box::new(ResolutionCondition {
                                left: ValueExpr::ObjectStat {
                                    subject: ValueObjectRef::AbilitySource,
                                    stat: ObjectStatKind::Toughness,
                                },
                                comparison: Comparison::LessOrEqual,
                                right: ValueExpr::literal(3),
                            }),
                        )),
                        grouping: TriggerGrouping::EachOccurrence,
                        body: ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::PutCountersOnSource { amount: 1.into() }],
                        ),
                    };
                    push_static_ability(
                        &mut p,
                        StaticContinuousAbility::Battlefield {
                            id: ids.static_id(),
                            selector: attached_sel,
                            parts: StaticParts(vec![StaticPart::GrantTrigger(Box::new(
                                attack_trig,
                            ))]),
                        },
                    )?;
                    "Young Hero"
                }
                _ => unreachable!(),
            };
            TokenDefinition::new(
                p,
                TokenName::Specified(
                    EnglishCardName::new(role_name.to_string())
                        .map_err(|_| "oracle:predef-token-invalid")?,
                ),
                vocabulary,
            )
            .map_err(|_| "oracle:predef-token-invalid")
        }
        _ => Err("oracle:unsupported-predef-token"),
    }
}
