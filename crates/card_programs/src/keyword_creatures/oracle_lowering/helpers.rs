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
        ValueExprAst::Count(f) => {
            matches!(
                f.zone,
                Some(ZoneFilter::ThatPlayerGraveyard | ZoneFilter::ThatPlayerHand)
            ) || matches!(
                f.controller,
                Some(
                    ControllerRelation::ThatPlayer
                        | ControllerRelation::ItsController
                        | ControllerRelation::OwnerThatPlayer
                )
            )
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
        InstructionAst::Draw { player, count, .. }
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
        }
        | InstructionAst::ForAsLongAs {
            condition,
            body: then_body,
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
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } => {
            value_expr_references_target_anaphora(x_expr)
                || value_expr_references_target_anaphora(y_expr)
                || body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::ForEachSubjectPlayer { players, body } => {
            matches!(players.as_ref(), SubjectRefAst::Player(p) if player_is_target_anaphora(*p))
                || body.iter().any(instruction_references_target_anaphora)
        }
        InstructionAst::CreateEmblemFor { recipient, .. } => {
            matches!(recipient.as_ref(), SubjectRefAst::Player(p) if player_is_target_anaphora(*p))
        }
        InstructionAst::RevealCards { subject } => {
            matches!(subject.as_ref(), SubjectRefAst::Player(p) if player_is_target_anaphora(*p))
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
        InstructionAst::MoveCounters { from, to, .. }
        | InstructionAst::MoveCounter { from, to, .. } => {
            subject_is_anaphora(from) || subject_is_anaphora(to)
        }
        InstructionAst::Attach {
            subject, target, ..
        } => subject_is_anaphora(subject) || subject_is_anaphora(target),
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
        | InstructionAst::TapPermanent { subject, .. }
        | InstructionAst::UntapPermanent { subject, .. }
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
        | InstructionAst::SetSuspected { subject, .. }
        | InstructionAst::TurnFaceDown { subject }
        | InstructionAst::RevealCards { subject }
        | InstructionAst::Connive { subject, .. }
        | InstructionAst::Exert { subject }
        | InstructionAst::ExileWithCounters { subject, .. }
        | InstructionAst::ShuffleIntoLibrary { subject }
        | InstructionAst::RemoveFromCombat { subject }
        | InstructionAst::DoubleStatOrCounters { subject, .. } => subject_is_anaphora(subject),
        InstructionAst::CreateEmblemFor { recipient, .. } => {
            subject_is_anaphora(recipient)
                || matches!(
                    recipient.as_ref(),
                    SubjectRefAst::ControllerOf(inner) | SubjectRefAst::OwnerOf(inner)
                        if subject_is_anaphora(inner)
                )
        }
        InstructionAst::Counter { target, .. }
        | InstructionAst::PreventDamage { target, .. }
        | InstructionAst::ChangeTargets { target, .. }
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
        | InstructionAst::BindXY { body: inner, .. }
        | InstructionAst::ForAsLongAs { body: inner, .. }
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
        InstructionAst::ForEachSubjectPlayer { players, body } => {
            subject_is_anaphora(players) || body.iter().any(instruction_references_anaphora)
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

fn value_expr_references_chosen_x(expr: &ValueExprAst) -> bool {
    match expr {
        ValueExprAst::ChosenX => true,
        ValueExprAst::Add(a, b)
        | ValueExprAst::Sub(a, b)
        | ValueExprAst::Mul(a, b)
        | ValueExprAst::AbsDiff(a, b) => {
            value_expr_references_chosen_x(a) || value_expr_references_chosen_x(b)
        }
        ValueExprAst::DivConst { expr, .. } => value_expr_references_chosen_x(expr),
        ValueExprAst::Pow { base, exp } => {
            value_expr_references_chosen_x(base) || value_expr_references_chosen_x(exp)
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            value_expr_references_chosen_x(threshold) || value_expr_references_chosen_x(actual)
        }
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
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } => Some(InstructionAst::Draw {
            player: *player,
            count: Box::new(scale_ast_amount(count, mult)),
            up_to: *up_to,
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
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } if !body.is_empty() && !value_expr_references_chosen_x(&mult) => {
            let scaled_body = body
                .iter()
                .map(|s| try_scale_instruction(s, mult.clone()))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::BindXY {
                x_expr: x_expr.clone(),
                y_expr: y_expr.clone(),
                body: scaled_body,
            })
        }
        InstructionAst::ForEachSubjectPlayer { players, body } if !body.is_empty() => {
            let scaled_body = body
                .iter()
                .map(|s| try_scale_instruction(s, mult.clone()))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::ForEachSubjectPlayer {
                players: players.clone(),
                body: scaled_body,
            })
        }
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
    let rewrite_that_player_controller = |f: &ObjectFilterAst| -> Option<ObjectFilterAst> {
        if f.controller != Some(ControllerRelation::ThatPlayer) {
            return None;
        }
        let mut out = f.clone();
        out.controller = match target_player {
            PlayerRefAst::EachPlayer => None,
            PlayerRefAst::EachOpponent => Some(ControllerRelation::Opponent),
            PlayerRefAst::You => Some(ControllerRelation::You),
            _ => return None,
        };
        Some(out)
    };
    let rewrite_that_player_zone_or_ctrl = |f: &ObjectFilterAst| -> Option<ObjectFilterAst> {
        let mut out = f.clone();
        let mut changed = false;
        if out.controller == Some(ControllerRelation::ThatPlayer) {
            out.controller = match target_player {
                PlayerRefAst::EachPlayer => None,
                PlayerRefAst::EachOpponent => Some(ControllerRelation::Opponent),
                PlayerRefAst::You => Some(ControllerRelation::You),
                _ => return None,
            };
            changed = true;
        }
        if out.zone == Some(ZoneFilter::ThatPlayerHand) {
            out.zone = Some(match target_player {
                PlayerRefAst::EachPlayer => ZoneFilter::EachPlayerHand,
                PlayerRefAst::EachOpponent => ZoneFilter::OpponentsHands,
                PlayerRefAst::You => ZoneFilter::YourHand,
                _ => return None,
            });
            changed = true;
        }
        changed.then_some(out)
    };
    match instr {
        InstructionAst::Draw {
            player,
            count,
            up_to,
        } if matches_loop_player(*player) => Some(InstructionAst::Draw {
            player: target_player,
            count: count.clone(),
            up_to: *up_to,
        }),
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
            except,
            half,
            rounding,
            chooser,
        } if matches_loop_player(*player) => Some(InstructionAst::Sacrifice {
            player: target_player,
            count: count.clone(),
            subject: subject.clone(),
            except: except.clone(),
            half: *half,
            rounding: *rounding,
            chooser: *chooser,
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
        InstructionAst::CreateEmblemFor {
            recipient,
            text,
            compiled,
        } if matches!(recipient.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) => {
            Some(InstructionAst::CreateEmblemFor {
                recipient: Box::new(SubjectRefAst::Player(target_player)),
                text: text.clone(),
                compiled: compiled.clone(),
            })
        }
        InstructionAst::RevealCards { subject } => match subject.as_ref() {
            SubjectRefAst::Player(p) if matches_loop_player(*p) => {
                Some(InstructionAst::RevealCards {
                    subject: Box::new(SubjectRefAst::Player(target_player)),
                })
            }
            SubjectRefAst::All(f) => Some(InstructionAst::RevealCards {
                subject: Box::new(SubjectRefAst::All(Box::new(
                    rewrite_that_player_zone_or_ctrl(f)?,
                ))),
            }),
            SubjectRefAst::Each(f) => Some(InstructionAst::RevealCards {
                subject: Box::new(SubjectRefAst::Each(Box::new(
                    rewrite_that_player_zone_or_ctrl(f)?,
                ))),
            }),
            _ => None,
        },
        InstructionAst::SetSuspected { subject, remove } => match subject.as_ref() {
            SubjectRefAst::All(f) => Some(InstructionAst::SetSuspected {
                subject: Box::new(SubjectRefAst::All(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
                remove: *remove,
            }),
            SubjectRefAst::Each(f) => Some(InstructionAst::SetSuspected {
                subject: Box::new(SubjectRefAst::Each(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
                remove: *remove,
            }),
            _ => None,
        },
        InstructionAst::TurnFaceDown { subject } => match subject.as_ref() {
            SubjectRefAst::All(f) => Some(InstructionAst::TurnFaceDown {
                subject: Box::new(SubjectRefAst::All(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
            }),
            SubjectRefAst::Each(f) => Some(InstructionAst::TurnFaceDown {
                subject: Box::new(SubjectRefAst::Each(Box::new(
                    rewrite_that_player_controller(f)?,
                ))),
            }),
            _ => None,
        },
        InstructionAst::BindXY {
            x_expr,
            y_expr,
            body,
        } if !body.is_empty()
            && !value_expr_references_target_anaphora(x_expr)
            && !value_expr_references_target_anaphora(y_expr) =>
        {
            let rewritten = body
                .iter()
                .map(|s| try_rewrite_for_each_player(s, target_player))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::BindXY {
                x_expr: x_expr.clone(),
                y_expr: y_expr.clone(),
                body: rewritten,
            })
        }
        InstructionAst::ForAsLongAs { condition, body }
            if !body.is_empty() && !condition_references_target_anaphora(condition) =>
        {
            let rewritten = body
                .iter()
                .map(|s| try_rewrite_for_each_player(s, target_player))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::ForAsLongAs {
                condition: condition.clone(),
                body: rewritten,
            })
        }
        InstructionAst::ForEachSubjectPlayer { players, body }
            if !body.is_empty()
                && matches!(players.as_ref(), SubjectRefAst::Player(p) if matches_loop_player(*p)) =>
        {
            let rewritten = body
                .iter()
                .map(|s| try_rewrite_for_each_player(s, target_player))
                .collect::<Option<Vec<_>>>()?;
            Some(InstructionAst::ForEachSubjectPlayer {
                players: Box::new(SubjectRefAst::Player(target_player)),
                body: rewritten,
            })
        }
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

fn bind_filter_player_target_local(
    builder: &mut BodyBuilder,
    filter: &ObjectFilter,
) -> Result<(), &'static str> {
    let mut needs_player = false;
    let mut needs_opponent = false;
    let mut inspect = |clause: ObjectClause| match clause.controller {
        ControllerFilter::TargetPlayer => needs_player = true,
        ControllerFilter::TargetOpponent => needs_opponent = true,
        _ => {}
    };
    match *filter {
        ObjectFilter::Clause(c) => inspect(c),
        ObjectFilter::Either(a, b) => {
            inspect(a);
            inspect(b);
        }
    }
    if needs_opponent {
        if builder.target != BodyTarget::SingleOpponent {
            builder.bind_target(BodyTarget::SingleOpponent)?;
        }
    } else if needs_player
        && !matches!(
            builder.target,
            BodyTarget::SinglePlayer | BodyTarget::SingleOpponent
        )
    {
        builder.bind_target(BodyTarget::SinglePlayer)?;
    }
    Ok(())
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
            if !self_pronoun_valid && !in_trigger {
                return Err("oracle:spell-cannot-modify-self");
            }
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
            bind_filter_player_target_local(builder, &obj)?;
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
            } else if !self_pronoun_valid {
                return Err("oracle:spell-cannot-modify-self");
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
            bind_filter_player_target_local(builder, &obj)?;
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
            up_to: false,
            any_number: false,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let plain_any = filter.is_empty()
                || (filter.any_card && filter.predicates.is_empty() && filter.flags.0 == 0);
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
        CostAtomAst::Sacrifice {
            count,
            filter,
            up_to: false,
            min_one: false,
        } => {
            let amt = lower_value_expr(resolve_x(count, bound_x), vocabulary)?;
            let ptf = lower_permanent_type_filter(filter, vocabulary)?;
            Ok(vec![Instruction::Sacrifice {
                players: AffectedPlayers::One(player),
                amount: amt,
                filter: ptf,
            }])
        }
        CostAtomAst::SacrificeAll(filter) => {
            if !matches!(filter.zone, None | Some(ZoneFilter::Battlefield))
                || !matches!(
                    filter.controller,
                    None | Some(ControllerRelation::You | ControllerRelation::ThatPlayer)
                )
            {
                return Err("oracle:unsupported-pay-cost");
            }
            let obj = lower_filter_any(filter)?;
            let scoped = match player {
                ResolvingPlayer::Controller => obj.controlled(),
                ResolvingPlayer::UniqueRelatedPlayer => obj.of_related_player(),
                ResolvingPlayer::TargetPlayer => obj.of_target_player(),
                _ => return Err("oracle:unsupported-pay-cost"),
            };
            Ok(vec![Instruction::SacrificeSubject {
                subject: Box::new(EffectSubject::Matching(scoped)),
            }])
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
                && filter.flags.0 == 0
                && filter.predicates.is_empty()
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
        CostAtomAst::Exile {
            filter,
            face_down: false,
            face_up: _,
            single_graveyard: false,
            any_number: true,
            min_one,
            total_mv: None,
            ..
        } => {
            let source_zone = match filter.zone {
                Some(ZoneFilter::YourHand | ZoneFilter::ThatPlayerHand) => SourceZoneFilter::Hand,
                Some(ZoneFilter::YourGraveyard | ZoneFilter::ThatPlayerGraveyard) => {
                    SourceZoneFilter::Graveyard
                }
                Some(ZoneFilter::OpponentGraveyard | ZoneFilter::OpponentsGraveyards) => {
                    SourceZoneFilter::OpponentGraveyard
                }
                Some(ZoneFilter::AnyGraveyard | ZoneFilter::AllGraveyards) => {
                    SourceZoneFilter::AnyGraveyard
                }
                Some(ZoneFilter::YourLibrary) => SourceZoneFilter::YourLibrary,
                Some(ZoneFilter::Exile) => SourceZoneFilter::Exile,
                None | Some(ZoneFilter::Battlefield) => SourceZoneFilter::Battlefield,
                _ => return Err("oracle:unsupported-pay-cost"),
            };
            let obj = lower_filter_any(filter)?;
            if obj.has_target_player() && player != ResolvingPlayer::TargetPlayer {
                return Err("oracle:unsupported-pay-cost");
            }
            Ok(vec![Instruction::SelectZoneCardsToZone {
                player,
                source_zone,
                filter: Box::new(obj),
                min: ValueExpr::literal(if *min_one { 1 } else { 0 }),
                max: ValueExpr::literal(999),
                destination: SubjectDestination::Exile {
                    delayed_return: None,
                },
                reveal: false,
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
        CostAtomAst::TapPermanent {
            count,
            filter,
            any_number: false,
        } => {
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

// Shared search-target binder for SearchLibrary lowering sites in body.rs and instructions.rs.
#[allow(dead_code)]
pub(super) fn bind_search_target(
    builder: &mut BodyBuilder,
    search: &LibrarySearch,
) -> Result<(), &'static str> {
    if search.source == SearchSource::TargetOpponentLibrary {
        builder.bind_target(BodyTarget::SingleOpponent)?;
    } else if search.source == SearchSource::TargetPlayerLibrary
        || search.searcher == ResolvingPlayer::TargetPlayer
        || search.curse_host() == Some(spellwake_engine::program::AuraSearchHost::TargetPlayer)
    {
        builder.bind_target(BodyTarget::SinglePlayer)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SearchLibraryExtra<'a> {
    pub(super) extra_zones: &'a [ZoneFilter],
    pub(super) secondary_filter: Option<&'a ObjectFilterAst>,
    pub(super) secondary_is_or: bool,
    pub(super) where_x: Option<&'a ValueExprAst>,
    pub(super) rest_to_zone: Option<&'a str>,
    pub(super) modifier: Option<&'a spellwake_oracle_compiler::ast::SearchModifierAst>,
}

fn filter_references_chosen_x(f: &ObjectFilterAst) -> bool {
    f.stat_bounds
        .iter()
        .any(|sb| value_expr_references_chosen_x(&sb.value))
}

fn substitute_chosen_x_in_value_expr(
    expr: &ValueExprAst,
    replacement: &ValueExprAst,
) -> (ValueExprAst, bool) {
    match expr {
        ValueExprAst::ChosenX => (replacement.clone(), true),
        ValueExprAst::Add(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Add(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::Sub(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Sub(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::Mul(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::Mul(Box::new(na), Box::new(nb)), ca || cb)
        }
        ValueExprAst::DivConst {
            expr: inner,
            denom,
            rounding,
        } => {
            let (ni, ci) = substitute_chosen_x_in_value_expr(inner, replacement);
            (
                ValueExprAst::DivConst {
                    expr: Box::new(ni),
                    denom: *denom,
                    rounding: *rounding,
                },
                ci,
            )
        }
        ValueExprAst::Pow { base, exp } => {
            let (nb, cb) = substitute_chosen_x_in_value_expr(base, replacement);
            let (ne, ce) = substitute_chosen_x_in_value_expr(exp, replacement);
            (
                ValueExprAst::Pow {
                    base: Box::new(nb),
                    exp: Box::new(ne),
                },
                cb || ce,
            )
        }
        ValueExprAst::Shortfall { threshold, actual } => {
            let (nt, ct) = substitute_chosen_x_in_value_expr(threshold, replacement);
            let (na, ca) = substitute_chosen_x_in_value_expr(actual, replacement);
            (
                ValueExprAst::Shortfall {
                    threshold: Box::new(nt),
                    actual: Box::new(na),
                },
                ct || ca,
            )
        }
        ValueExprAst::AbsDiff(a, b) => {
            let (na, ca) = substitute_chosen_x_in_value_expr(a, replacement);
            let (nb, cb) = substitute_chosen_x_in_value_expr(b, replacement);
            (ValueExprAst::AbsDiff(Box::new(na), Box::new(nb)), ca || cb)
        }
        _ => (expr.clone(), false),
    }
}

fn substitute_chosen_x_in_filter(
    filter: &ObjectFilterAst,
    replacement: &ValueExprAst,
) -> Option<ObjectFilterAst> {
    let mut out = filter.clone();
    let mut changed = false;
    for sb in &mut out.stat_bounds {
        let (nv, c) = substitute_chosen_x_in_value_expr(&sb.value, replacement);
        if c {
            *sb.value = nv;
            changed = true;
        }
    }
    changed.then_some(out)
}

fn extract_single_basic_land_type(f: &ObjectFilterAst) -> Option<LandType> {
    if f.subtypes.len() != 1
        || !(f.any_types.is_empty() || f.any_types == AstCardTypeMask::LAND)
        || !f.forbidden_types.is_empty()
        || !f.required_supertypes.is_empty()
        || !f.forbidden_supertypes.is_empty()
        || !f.forbidden_subtypes.is_empty()
        || !f.any_colors.is_empty()
        || !f.forbidden_colors.is_empty()
        || f.color_arity.is_some()
        || f.controller.is_some()
        || f.zone.is_some()
        || f.combat.is_some()
        || f.tapped.is_some()
        || f.token.is_some()
        || !f.required_keywords.is_empty()
        || !f.forbidden_keywords.is_empty()
        || !f.stat_bounds.is_empty()
        || f.counter_kind.is_some()
        || f.named.is_some()
        || !f.flags.is_empty()
        || !f.predicates.is_empty()
        || f.any_spell
        || f.is_ability
    {
        return None;
    }
    match parse_basic_land_subtype(&f.subtypes[0])? {
        BasicLandType::Plains => Some(LandType::Plains),
        BasicLandType::Island => Some(LandType::Island),
        BasicLandType::Swamp => Some(LandType::Swamp),
        BasicLandType::Mountain => Some(LandType::Mountain),
        BasicLandType::Forest => Some(LandType::Forest),
    }
}

// Internal lowering helper taking the orthogonal fields of `InstructionAst::SearchLibrary` plus lowering flags.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_library_search_ext(
    library: &str,
    count: &ValueExprAst,
    filter: &ObjectFilterAst,
    reveal: bool,
    to_zone: &str,
    vocabulary: &CreatureTypeVocabulary,
    optional_search: bool,
    up_to: bool,
    any_number: bool,
    extra: &SearchLibraryExtra<'_>,
) -> Result<LibrarySearch, &'static str> {
    if extra.modifier.is_some() {
        return Err("oracle:unsupported-search-modifier");
    }

    let lib_low = library.trim().to_ascii_lowercase();
    let (searcher, mut source) = match lib_low.as_str() {
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
        "target player" | "target player's" | "target player's library" => (
            ResolvingPlayer::Controller,
            SearchSource::TargetPlayerLibrary,
        ),
        _ => return Err("oracle:unsupported-search-source"),
    };
    if !extra.extra_zones.is_empty() {
        if source == SearchSource::OwnLibrary && extra.extra_zones == [ZoneFilter::YourGraveyard] {
            source = SearchSource::OwnLibraryOrGraveyard;
        } else {
            return Err("oracle:unsupported-search-source");
        }
    }

    let mut substituted_filter_owned: Option<ObjectFilterAst> = None;
    let mut substituted_secondary_owned: Option<ObjectFilterAst> = None;
    let mut where_x_count: Option<SearchCount> = None;
    if let Some(where_expr) = extra.where_x {
        if any_number || value_expr_references_chosen_x(where_expr) {
            return Err("oracle:unsupported-search-where-x");
        }
        if matches!(count, ValueExprAst::ChosenX) {
            let ValueExprAst::CountPlayersMatching { predicate } = where_expr else {
                return Err("oracle:unsupported-search-where-x");
            };
            let pred_low = predicate.trim().to_ascii_lowercase();
            let is_more_lands = matches!(
                pred_low.as_str(),
                "the number of players who control more lands than you"
                    | "players who control more lands than you"
                    | "the number of opponents who control more lands than you"
                    | "opponents who control more lands than you"
            );
            if !is_more_lands
                || filter_references_chosen_x(filter)
                || extra
                    .secondary_filter
                    .is_some_and(filter_references_chosen_x)
            {
                return Err("oracle:unsupported-search-where-x");
            }
            where_x_count = Some(SearchCount::UpToPlayersWithMoreLands);
        } else {
            let mut substituted = false;
            if let Some(sub_f) = substitute_chosen_x_in_filter(filter, where_expr) {
                substituted_filter_owned = Some(sub_f);
                substituted = true;
            }
            if let Some(sec) = extra.secondary_filter
                && let Some(sub_s) = substitute_chosen_x_in_filter(sec, where_expr)
            {
                substituted_secondary_owned = Some(sub_s);
                substituted = true;
            }
            if !substituted {
                return Err("oracle:unsupported-search-where-x");
            }
        }
    }
    let effective_filter = substituted_filter_owned.as_ref().unwrap_or(filter);
    let effective_secondary = substituted_secondary_owned
        .as_ref()
        .or(extra.secondary_filter);

    let (matcher, pair_constraint, pair_count_override) = if let Some(sec) = effective_secondary {
        if extra.secondary_is_or {
            let first = lower_object_filter(effective_filter, vocabulary)?;
            let second = lower_object_filter(sec, vocabulary)?;
            let (ObjectFilter::Clause(c1), ObjectFilter::Clause(c2)) = (first, second) else {
                return Err("oracle:unsupported-secondary-search-filter");
            };
            (
                SearchCardMatcher::Filtered(Box::new(ObjectFilter::Either(c1, c2))),
                None,
                None,
            )
        } else {
            if any_number || where_x_count.is_some() || !matches!(count, ValueExprAst::Const(1 | 2))
            {
                return Err("oracle:unsupported-secondary-search-filter");
            }
            let (Some(sub1), Some(sub2)) = (
                extract_single_basic_land_type(effective_filter),
                extract_single_basic_land_type(sec),
            ) else {
                return Err("oracle:unsupported-secondary-search-filter");
            };
            if sub1 == sub2 {
                return Err("oracle:unsupported-secondary-search-filter");
            }
            (
                SearchCardMatcher::LandSubtypes {
                    subtypes: BTreeSet::from([sub1, sub2]),
                    require_land_type: true,
                },
                Some(
                    spellwake_engine::program::SearchPairConstraint::DistinctLandTypes(sub1, sub2),
                ),
                Some(SearchCount::UpTo(2)),
            )
        }
    } else {
        (
            lower_search_matcher(effective_filter, vocabulary)?,
            None,
            None,
        )
    };

    let search_count = if let Some(pair_cnt) = pair_count_override {
        pair_cnt
    } else if let Some(wx_cnt) = where_x_count {
        wx_cnt
    } else if any_number {
        SearchCount::UpTo(u8::MAX)
    } else {
        let ValueExprAst::Const(n) = count else {
            return Err("oracle:dynamic-search-count");
        };
        let n_u8 = u8::try_from(*n).map_err(|_| "oracle:search-count-ovf")?;
        if n_u8 == 0 {
            return Err("oracle:search-zero");
        }
        if matcher == SearchCardMatcher::Any && !optional_search && !up_to {
            SearchCount::Exact(NonZeroU8::new(n_u8).ok_or("oracle:search-zero")?)
        } else {
            SearchCount::UpTo(n_u8)
        }
    };

    let dest_low = to_zone.to_ascii_lowercase();
    let mut destination = if dest_low.contains("hand") {
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
    if let Some(rest) = extra.rest_to_zone {
        let is_count_two = matches!(search_count, SearchCount::UpTo(2))
            || matches!(search_count, SearchCount::Exact(n) if n.get() == 2);
        if !is_count_two {
            return Err("oracle:unsupported-search-dest");
        }
        let rest_norm = rest
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_ascii_lowercase();
        let rest_is_hand = matches!(
            rest_norm.as_str(),
            "hand"
                | "your hand"
                | "into your hand"
                | "in your hand"
                | "their hand"
                | "into their hand"
                | "that player's hand"
                | "into that player's hand"
        );
        let rest_is_graveyard = matches!(
            rest_norm.as_str(),
            "graveyard"
                | "your graveyard"
                | "into your graveyard"
                | "their graveyard"
                | "into their graveyard"
                | "that player's graveyard"
                | "into that player's graveyard"
        );
        destination = match destination {
            SearchDestination::Battlefield {
                tapped: true,
                post_entry: spellwake_engine::program::SearchBattlefieldPostEntry::None,
            } if rest_is_hand => SearchDestination::SplitBattlefieldTappedAndHand,
            SearchDestination::Hand if rest_is_graveyard => {
                SearchDestination::SplitHandAndGraveyard
            }
            _ => return Err("oracle:unsupported-search-dest"),
        };
    }

    let mut ls = LibrarySearch {
        searcher,
        source,
        matcher,
        mana_value: None,
        count: search_count,
        pair_constraint,
        destination,
        reveal,
    };
    if !matches!(ls.matcher, SearchCardMatcher::Filtered(_)) {
        if effective_filter.stat_bounds.len() == 1 {
            let sb = &effective_filter.stat_bounds[0];
            if sb.stat == StatKind::ManaValue {
                match (sb.op, sb.value.as_ref()) {
                    (CompareOp::Le, ValueExprAst::Const(v)) if *v >= 0 => {
                        ls.mana_value = Some(SearchManaValue::AtMost(
                            u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?,
                        ));
                    }
                    (CompareOp::Ge, ValueExprAst::Const(v)) if *v >= 0 => {
                        ls.mana_value = Some(SearchManaValue::AtLeast(
                            u32::try_from(*v).map_err(|_| "oracle:mv-ovf")?,
                        ));
                    }
                    (CompareOp::Eq, ValueExprAst::Const(v)) if *v >= 0 => {
                        ls.mana_value = Some(SearchManaValue::Exact(i64::from(*v).into()));
                    }
                    (CompareOp::Le, ValueExprAst::ChosenX) => {
                        ls.mana_value = Some(SearchManaValue::AtMostAnnouncedX);
                    }
                    (_, ValueExprAst::ChosenX) => {
                        return Err("oracle:unsupported-search-mv");
                    }
                    (
                        CompareOp::Eq,
                        ValueExprAst::CounterCount {
                            subject,
                            counter_kind,
                        },
                    ) if matches!(subject.as_ref(), SubjectRefAst::SelfObject) => {
                        let ck = parse_counter_kind(counter_kind)
                            .filter(|c| c.is_permanent())
                            .ok_or("oracle:unsupported-search-mv")?;
                        ls.mana_value = Some(SearchManaValue::EqualToSourceCounters(ck));
                    }
                    _ => {
                        let obj = lower_object_filter(effective_filter, vocabulary)?;
                        ls.matcher = SearchCardMatcher::Filtered(Box::new(obj));
                    }
                }
            } else {
                let obj = lower_object_filter(effective_filter, vocabulary)?;
                ls.matcher = SearchCardMatcher::Filtered(Box::new(obj));
            }
        } else if !effective_filter.stat_bounds.is_empty() {
            let obj = lower_object_filter(effective_filter, vocabulary)?;
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
        .replace("This artifact", "~")
        .replace("this enchantment", "~")
        .replace("This enchantment", "~")
        .replace("this land", "~")
        .replace("This land", "~")
        .replace("this equipment", "~")
        .replace("This equipment", "~")
        .replace("this aura", "~")
        .replace("This aura", "~");
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

fn lower_token_face_ast(
    face_ast: &FaceAst,
    is_aura: bool,
    program: &mut Program,
    vocabulary: &CreatureTypeVocabulary,
    tok_ids: &mut LoweringIds,
) -> Result<(), &'static str> {
    if is_aura
        && program.permanent().and_then(|d| d.attachment).is_none()
        && face_ast
            .statics
            .iter()
            .any(|st| matches!(st, StaticAbilityAst::AttachmentRestriction { .. }))
    {
        let restriction = find_aura_restriction(&face_ast.statics, vocabulary)
            .map_err(|_| "oracle:unsupported-granted-keyword")?;
        if let ProgramKind::Permanent(def) = &mut program.kind {
            def.attachment = Some(AttachmentKind::Aura(restriction));
        }
    }
    for (k_idx, kw_ast) in face_ast.keywords.iter().enumerate() {
        lower_keyword(kw_ast, k_idx, face_ast, program, vocabulary, tok_ids, false)
            .map_err(|_| "oracle:unsupported-granted-keyword")?;
    }
    for st in &face_ast.statics {
        lower_static(st, program, vocabulary, tok_ids, false)
            .map_err(|_| "oracle:unsupported-granted-keyword")?;
    }
    for rep in &face_ast.replacements {
        lower_replacement(rep, program, vocabulary, tok_ids)
            .map_err(|_| "oracle:unsupported-granted-keyword")?;
    }
    for ab in &face_ast.abilities {
        lower_ability(ab, program, vocabulary, tok_ids)
            .map_err(|_| "oracle:unsupported-granted-keyword")?;
    }
    Ok(())
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

            let mut creature_subtypes = BTreeSet::new();
            let mut artifact_subtypes = BTreeSet::new();
            let mut enchantment_subtypes = BTreeSet::new();
            let mut land_subtypes = BTreeSet::new();
            let mut ordered_subtypes = Vec::new();
            let mut is_aura = false;
            for st in subtypes {
                if st.eq_ignore_ascii_case("aura") {
                    is_aura = true;
                } else if let Some(ct) = vocabulary.resolve_label(st) {
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
                } else if let Some(lt) =
                    LandType::from_wire_label(st).or_else(|| parse_land_subtype(st))
                {
                    if land_subtypes.insert(lt) {
                        ordered_subtypes.push(TokenSubtype::Land(lt));
                    }
                } else {
                    return Err("oracle:token-unknown-subtype");
                }
            }

            let is_noncreature = power.is_empty()
                && toughness.is_empty()
                && (card_types.0 & AstCardTypeMask::CREATURE.0) == 0;
            let (mut p, base_size) = if is_noncreature {
                if *attacking {
                    return Err("oracle:noncreature-attacking-token");
                }
                let mut perm_types = card_type_mask_to_set(*card_types)?;
                if perm_types.is_empty()
                    || perm_types
                        .iter()
                        .any(|t| matches!(t, CardType::Instant | CardType::Sorcery))
                    || !perm_types.iter().copied().any(CardType::is_permanent)
                {
                    return Err("oracle:token-nonpermanent-type");
                }
                if is_aura {
                    perm_types.insert(CardType::Enchantment);
                }
                if artifact_subtypes.contains(&ArtifactSubtype::Equipment)
                    || artifact_subtypes.contains(&ArtifactSubtype::Fortification)
                {
                    perm_types.insert(CardType::Artifact);
                }
                let attachment = if artifact_subtypes.contains(&ArtifactSubtype::Equipment) {
                    Some(AttachmentKind::Equipment)
                } else if artifact_subtypes.contains(&ArtifactSubtype::Fortification) {
                    Some(AttachmentKind::Fortification)
                } else {
                    None
                };
                let prog = Program::new(
                    perm_types,
                    creature_subtypes,
                    land_subtypes,
                    ProgramKind::Permanent(PermanentDefinition {
                        entry_tap: None,
                        attachment,
                        printed_pt: None,
                        casting_cost: None,
                    }),
                );
                (prog, None)
            } else {
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
                let mut prog =
                    Program::creature(None, p_val.into(), t_val.into(), creature_subtypes);
                let extra_types = card_type_mask_to_set(*card_types)?;
                for t in extra_types {
                    if t.is_permanent() {
                        prog.card_types.insert(t);
                    }
                }
                prog.land_subtypes = land_subtypes;
                if artifact_subtypes.contains(&ArtifactSubtype::Equipment)
                    && let ProgramKind::Permanent(def) = &mut prog.kind
                {
                    def.attachment = Some(AttachmentKind::Equipment);
                }
                (prog, base_size)
            };
            p.supertypes = supertype_mask_to_set(*supertypes)?;
            p.artifact_subtypes = artifact_subtypes;
            p.enchantment_subtypes = enchantment_subtypes;
            p.effect_defined_colors = Some(color_mask_to_set(*colors));

            let mut type_words: Vec<&str> = Vec::new();
            if p.supertypes.contains(&Supertype::Legendary) {
                type_words.push("Legendary");
            }
            for ct in &p.card_types {
                match ct {
                    CardType::Artifact => type_words.push("Artifact"),
                    CardType::Battle => type_words.push("Battle"),
                    CardType::Creature => type_words.push("Creature"),
                    CardType::Enchantment => type_words.push("Enchantment"),
                    CardType::Kindred => type_words.push("Kindred"),
                    CardType::Land => type_words.push("Land"),
                    CardType::Planeswalker => type_words.push("Planeswalker"),
                    CardType::Instant | CardType::Sorcery => {}
                }
            }
            if is_aura {
                type_words.push("Aura");
            }
            if p.artifact_subtypes.contains(&ArtifactSubtype::Equipment) {
                type_words.push("Equipment");
            }
            let tok_type_line = if type_words.is_empty() {
                "Creature".to_string()
            } else {
                type_words.join(" ")
            };

            let mut tok_ids = LoweringIds::default();
            let compiler_vocab = cached_compiler_vocab(vocabulary);
            let tok_name = named.as_deref().unwrap_or("Token");
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
                            let norm_part = normalize_token_ability_text(trimmed, tok_name);
                            let parsed_kw = compile_card(
                                &CardInput {
                                    oracle_id: "",
                                    name: tok_name,
                                    layout: "normal",
                                    faces: vec![FaceInput {
                                        name: tok_name,
                                        type_line: &tok_type_line,
                                        oracle_text: &norm_part,
                                    }],
                                },
                                &compiler_vocab,
                            );
                            if !parsed_kw.strictly_compiled {
                                return Err("oracle:unsupported-granted-keyword");
                            }
                            if let Some(face_ast) = parsed_kw.faces.first() {
                                lower_token_face_ast(
                                    face_ast,
                                    is_aura,
                                    &mut p,
                                    vocabulary,
                                    &mut tok_ids,
                                )?;
                            }
                        }
                    }
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
                                    type_line: &tok_type_line,
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
                        lower_token_face_ast(face_ast, is_aura, &mut p, vocabulary, &mut tok_ids)?;
                    }
                } else if let Some(kw) = lower_keyword_ability_name(kw_raw) {
                    p.printed_keywords.insert(kw);
                } else {
                    let norm_kw = normalize_token_ability_text(kw_raw, tok_name);
                    let compiled_kw = compile_card(
                        &CardInput {
                            oracle_id: "",
                            name: tok_name,
                            layout: "normal",
                            faces: vec![FaceInput {
                                name: tok_name,
                                type_line: &tok_type_line,
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
                    lower_token_face_ast(face_ast, is_aura, &mut p, vocabulary, &mut tok_ids)?;
                }
            }
            if is_aura
                && !matches!(
                    p.permanent().and_then(|d| d.attachment),
                    Some(AttachmentKind::Aura(_))
                )
            {
                return Err("oracle:aura-missing-enchant");
            }

            let token_name = if let Some(n) = named {
                TokenName::Specified(
                    EnglishCardName::new(n.clone()).map_err(|_| "oracle:token-name")?,
                )
            } else if !ordered_subtypes.is_empty() {
                TokenName::Default { ordered_subtypes }
            } else if is_aura {
                TokenName::Specified(
                    EnglishCardName::new("Aura Token".to_string())
                        .map_err(|_| "oracle:token-name")?,
                )
            } else {
                return Err("oracle:nameless-token");
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
