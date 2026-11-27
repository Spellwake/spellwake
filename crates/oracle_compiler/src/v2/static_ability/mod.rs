//! Strictly-typed [`StaticAbility`] parser for `v2`.

use crate::ast::TurnStepAst;
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{
    parse_keyword_list, parse_object_filter, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    CastPermissionMode, ConditionExpr, ConditionFlag, EventCheckKind, FilterPredicate,
    FilterStaticKind, HandSizeRule, LevelMarker, ObjectFilter, PlayerRef, PlayerRestrictionKind,
    RuleRestrictionKind, StaticAbility, StaticFlagKind, SubjectRef, SubjectStaticRule,
    TriggerModifierKind, ValueExpr, VarName, VisibilityKind,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse one strictly-typed [`StaticAbility`] sentence (fail-closed: zero leftover tokens).
pub(crate) mod cond_ext;
mod cost_modifiers;
mod global_rules;
mod player_rules;
mod subject_rules;
mod subject_rules_ext;
use cond_ext::{parse_static_condition, parse_static_scale_expr, parse_static_value_expr};
use cost_modifiers::{parse_cost_modifier, parse_next_spell_modifier};
use global_rules::parse_global_static_ability;
use player_rules::parse_player_static_ability;

pub(super) fn and_cond(prev: Option<ConditionExpr>, extra: ConditionExpr) -> ConditionExpr {
    match prev {
        Some(p) => ConditionExpr::And(Box::new(p), Box::new(extra)),
        None => extra,
    }
}

pub(super) fn merge_cond(
    prev: Option<ConditionExpr>,
    extra: Option<ConditionExpr>,
) -> Option<ConditionExpr> {
    match extra {
        Some(c) => Some(and_cond(prev, c)),
        None => prev,
    }
}

pub(super) fn wrap_compound(mut parts: Vec<StaticAbility>) -> StaticAbility {
    if parts.len() == 1 {
        parts.remove(0)
    } else {
        StaticAbility::Compound(parts)
    }
}

fn try_trailing_phrase_cond(
    ts: &mut TokenStream<'_>,
    end_trim: usize,
    words: &[&str],
    extra_cond: ConditionExpr,
    condition: &Option<ConditionExpr>,
) -> Option<StaticAbility> {
    let n = words.len();
    if end_trim < ts.pos + n {
        return None;
    }
    let matched = words.iter().enumerate().all(|(i, &w)| {
        matches!(
            ts.tokens[end_trim - n + i].kind,
            crate::v2::lexer::TokenKind::Word(tok) if tok.eq_ignore_ascii_case(w)
        )
    });
    if !matched {
        return None;
    }
    let mut body_ts = ts.sub_stream(&ts.tokens[ts.pos..end_trim - n], ts.stage);
    let merged = and_cond(condition.clone(), extra_cond);
    if let Ok(st) = parse_static_core(&mut body_ts, Some(merged))
        && body_ts.is_eof()
    {
        ts.commit_substream_eof(ts.tokens.len(), &body_ts);
        return Some(st);
    }
    None
}

fn try_trailing_unless(
    ts: &mut TokenStream<'_>,
    body_start: usize,
    end_trim: usize,
    base_cond: Option<ConditionExpr>,
) -> Option<StaticAbility> {
    let u_idx = (body_start + 1..end_trim).find(|&idx| {
        matches!(
            ts.tokens[idx].kind,
            crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("unless")
        )
    })?;
    let mut cond_ts = ts.sub_stream(&ts.tokens[u_idx + 1..end_trim], ts.stage);
    let c = parse_static_condition(&mut cond_ts).ok()?;
    if !cond_ts.is_eof() {
        return None;
    }
    let merged = and_cond(base_cond, ConditionExpr::Not(Box::new(c)));
    let mut body_ts = ts.sub_stream(&ts.tokens[body_start..u_idx], ts.stage);
    let st = parse_static_core(&mut body_ts, Some(merged)).ok()?;
    if body_ts.is_eof() {
        ts.commit_substream_eof(ts.tokens.len(), &body_ts);
        Some(st)
    } else {
        None
    }
}

pub fn parse_static_ability(ts: &mut TokenStream<'_>) -> PResult<StaticAbility> {
    // Optional leading (possibly stacked!) `During your turn, `, `As long as <condition>, `, or `If <condition>, `
    let mut condition: Option<ConditionExpr> = None;
    loop {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["during", "each", "player's", "turn"]) && ts.eat_comma() {
            continue;
        }
        ts.restore(lcp);
        let next_cond = if ts.eat_phrase(&["as", "long", "as"]) {
            let c = parse_static_condition(ts)?;
            if !ts.eat_comma() {
                return Err(ts.err("expected `,` after `As long as <condition>`"));
            }
            Some(c)
        } else if ts.eat_any_phrase(&[
            &["during", "your", "turn"],
            &["during", "each", "of", "your", "turns"],
        ]) && ts.eat_comma()
        {
            Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn))
        } else if ts.eat_phrase(&["during", "turns", "other", "than", "yours"]) && ts.eat_comma() {
            Some(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                ConditionFlag::IsYourTurn,
            ))))
        } else if ts.eat_phrase(&["during", "combat"]) && ts.eat_comma() {
            Some(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
                TurnStepAst::BeginningOfCombat,
            )))
        } else if ts.eat_phrase(&["during", "your", "end", "step"]) && ts.eat_comma() {
            Some(ConditionExpr::And(
                Box::new(ConditionExpr::Flag(ConditionFlag::IsYourTurn)),
                Box::new(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
                    TurnStepAst::EndStep,
                ))),
            ))
        } else if ts.eat_phrase(&["during", "each", "opponent's", "end", "step"]) && ts.eat_comma()
        {
            Some(ConditionExpr::And(
                Box::new(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                    ConditionFlag::IsYourTurn,
                )))),
                Box::new(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
                    TurnStepAst::EndStep,
                ))),
            ))
        } else {
            ts.restore(lcp);
            let icp = ts.checkpoint();
            if ts.eat_word("if")
                && let Ok(c) = try_parse(ts, parse_static_condition)
                && ts.eat_comma()
            {
                Some(c)
            } else {
                ts.restore(icp);
                None
            }
        };
        match next_cond {
            Some(c) => condition = Some(and_cond(condition, c)),
            None => break,
        }
    }

    // Strip trailing `as long as <condition>` if present!
    let end_trim = ts.tokens.len()
        - usize::from(matches!(
            ts.tokens.last().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Period)
        ));

    // Strip trailing `, where X is <ValueExpr>` binder (including comma inside preceding quote)
    if let Some((body_end, expr_start)) = find_where_x_is(ts.tokens, ts.pos, end_trim) {
        let mut expr_ts = ts.sub_stream(&ts.tokens[expr_start..end_trim], ts.stage);
        if let Ok(expr) = parse_value_expr(&mut expr_ts)
            && expr_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(&ts.tokens[ts.pos..body_end], ts.stage);
            let inner = parse_static_core(&mut body_ts, condition.take())?;
            body_ts.expect_eof()?;
            ts.commit_substream_eof(ts.tokens.len(), &body_ts);
            return Ok(StaticAbility::BindVariable {
                var: VarName::X,
                expr,
                inner: Box::new(inner),
            });
        }
    }

    if condition.is_none()
        && let Some(ala_idx) = find_as_long_as(ts.tokens, ts.pos, end_trim)
        && ala_idx > ts.pos
    {
        let mut cond_ts = ts.sub_stream(&ts.tokens[ala_idx + 3..end_trim], ts.stage);
        if let Ok(c) = parse_static_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(&ts.tokens[ts.pos..ala_idx], ts.stage);
            let st = parse_static_core(&mut body_ts, Some(c))?;
            body_ts.expect_eof()?;
            ts.commit_substream_eof(ts.tokens.len(), &body_ts);
            return Ok(st);
        }
    }

    // Strip trailing `during your turn` or `during combat` (merging with any leading condition)
    if let Some(st) = try_trailing_phrase_cond(
        ts,
        end_trim,
        &["during", "your", "turn"],
        ConditionExpr::Flag(ConditionFlag::IsYourTurn),
        &condition,
    ) {
        return Ok(st);
    }
    if let Some(st) = try_trailing_phrase_cond(
        ts,
        end_trim,
        &["during", "combat"],
        ConditionExpr::EventCheck(EventCheckKind::DuringStep(TurnStepAst::BeginningOfCombat)),
        &condition,
    ) {
        return Ok(st);
    }

    // Strip trailing `if <cond>` on static abilities
    if condition.is_none()
        && let Some(if_idx) = (ts.pos + 1..end_trim)
            .find(|&idx| matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("if")))
    {
        let mut cond_ts = ts.sub_stream(&ts.tokens[if_idx + 1..end_trim], ts.stage);
        if let Ok(c) = parse_static_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let mut body_ts = ts.sub_stream(&ts.tokens[ts.pos..if_idx], ts.stage);
            if let Ok(st) = parse_static_core(&mut body_ts, Some(c))
                && body_ts.is_eof()
            {
                ts.commit_substream_eof(ts.tokens.len(), &body_ts);
                return Ok(st);
            }
        }
    }

    if let Some(cond) = condition.clone() {
        let rcp = ts.checkpoint();
        if let Ok(rep) = try_parse(ts, crate::v2::replacement::parse_replacement) {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(StaticAbility::ConditionalReplacement {
                    condition: cond,
                    replacement: Box::new(rep),
                });
            }
        }
        ts.restore(rcp);
    }

    let base_cond = condition.clone();
    let core_cp = ts.checkpoint();
    let first_res = parse_static_core(ts, condition.take());
    let mut parts = match first_res {
        Ok(st) => vec![st],
        Err(first_err) => {
            // Fallback: trailing `unless <cond>` when the direct parse failed
            ts.restore(core_cp);
            if let Some(st) = try_trailing_unless(ts, ts.pos, end_trim, base_cond.clone()) {
                return Ok(st);
            }
            return Err(first_err);
        }
    };
    // Compositional `[, ] and <second static ability>`
    loop {
        let ccp = ts.checkpoint();
        let had_comma = ts.eat_comma();
        if ts.eat_word("and") {
            if let Ok(next) = try_parse(ts, |t| parse_static_core(t, base_cond.clone())) {
                parts.push(next);
                continue;
            }
            if had_comma
                && (base_cond.is_none()
                    || ts.peek_word().is_some_and(|w| {
                        matches!(w.to_ascii_lowercase().as_str(), "during" | "as" | "if")
                    }))
                && let Ok(next) = try_parse(ts, parse_static_ability)
            {
                parts.push(next);
                break;
            }
        }
        ts.restore(ccp);
        break;
    }
    let _ = ts.eat_period();
    // Fallback: trailing `unless <cond>` left unconsumed after a valid prefix
    if !ts.is_eof()
        && let Some(st) = try_trailing_unless(ts, core_cp_pos(ts, core_cp), end_trim, base_cond)
    {
        return Ok(st);
    }
    ts.expect_eof()?;
    Ok(wrap_compound(parts))
}

fn core_cp_pos(ts: &TokenStream<'_>, cp: crate::v2::parser::Checkpoint) -> usize {
    let mut probe = ts.clone();
    probe.restore(cp);
    probe.pos
}

fn find_where_x_is(
    tokens: &[crate::v2::lexer::Token<'_>],
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    use crate::v2::lexer::TokenKind;
    if end < start + 5 {
        return None;
    }
    for i in start..=end - 5 {
        let sep_body_end = match tokens[i].kind {
            TokenKind::Comma => Some(i),
            TokenKind::Quoted(q) if q.trim_end().ends_with(',') => Some(i + 1),
            _ => None,
        };
        if let Some(body_end) = sep_body_end
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("where"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x"))
            && matches!(tokens[i + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("is"))
        {
            return Some((body_end, i + 4));
        }
    }
    None
}

fn find_as_long_as(
    tokens: &[crate::v2::lexer::Token<'_>],
    start: usize,
    end: usize,
) -> Option<usize> {
    use crate::v2::lexer::TokenKind;
    if end < start + 3 {
        return None;
    }
    for i in start..=end - 3 {
        if matches!(tokens[i].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("as"))
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("long"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("as"))
        {
            return Some(i);
        }
    }
    None
}

fn ability_preserves_condition(st: &StaticAbility) -> bool {
    match st {
        StaticAbility::ContinuousModifyPT { condition, .. }
        | StaticAbility::GrantKeywords { condition, .. }
        | StaticAbility::GrantAbility { condition, .. }
        | StaticAbility::CostModifier { condition, .. }
        | StaticAbility::RuleRestriction { condition, .. }
        | StaticAbility::PlayerRestriction { condition, .. }
        | StaticAbility::EntersWithCountersStatic { condition, .. }
        | StaticAbility::ActivationCostModifier { condition, .. }
        | StaticAbility::CantCastSpellsMatching { condition, .. }
        | StaticAbility::ZoneCastPermission { condition, .. }
        | StaticAbility::SubjectRule { condition, .. } => condition.is_some(),
        StaticAbility::ConditionalReplacement { .. } => true,
        StaticAbility::BindVariable { inner, .. } => ability_preserves_condition(inner),
        StaticAbility::NextSpellModifier { effect, .. } => ability_preserves_condition(effect),
        StaticAbility::Compound(parts) => {
            !parts.is_empty() && parts.iter().all(ability_preserves_condition)
        }
        StaticAbility::SkipStep { .. }
        | StaticAbility::SetSubtypes { .. }
        | StaticAbility::RemoveAllAbilities { .. }
        | StaticAbility::SetBasePT { .. }
        | StaticAbility::AdditionalCost(_)
        | StaticAbility::AlternativeCost(_)
        | StaticAbility::DynamicBasePT { .. }
        | StaticAbility::ControlEnchanted { .. }
        | StaticAbility::PreventDamageStatic { .. }
        | StaticAbility::AdditionalLandPlayStatic { .. }
        | StaticAbility::MaxAttackersPerCombat { .. }
        | StaticAbility::RevealOpeningHand { .. }
        | StaticAbility::LegendRuleDoesntApply(_)
        | StaticAbility::AttackTax { .. }
        | StaticAbility::VisibilityRule { .. }
        | StaticAbility::TriggerModifier { .. }
        | StaticAbility::HandSize { .. }
        | StaticAbility::GlobalFlag(_)
        | StaticAbility::LevelStructure(_)
        | StaticAbility::FilterRule { .. } => false,
    }
}

fn parse_static_core(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    let cp = ts.checkpoint();
    let had_cond = condition.is_some();
    let st = parse_static_core_inner(ts, condition)?;
    if had_cond && !ability_preserves_condition(&st) {
        ts.restore(cp);
        return Err(ts.err("static ability IR variant cannot preserve condition"));
    }
    Ok(st)
}

fn parse_static_core_inner(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 0. `As an additional cost to cast this spell/~, <cost_list>`
    // Disjunctive `or <cost>` is NOT coerced into conjunctive `AdditionalCost(Vec<Cost>)`;
    // fail closed if `parse_cost_list` does not consume the entire cost clause.
    {
        let ac_cp = ts.checkpoint();
        if ts.eat_phrase(&["as", "an", "additional", "cost", "to", "cast"])
            && (ts.eat_phrase(&["this", "spell"]) || ts.eat_tilde())
            && ts.eat_comma()
            && let Ok(costs) = try_parse(ts, parse_cost_list)
            && !costs.is_empty()
        {
            return Ok(StaticAbility::AdditionalCost(costs));
        }
        ts.restore(ac_cp);
    }

    // 00. Global, combat-rule, and extended keyword statics (`global_rules.rs`)
    if let Ok(st) = try_parse(ts, |s| parse_global_static_ability(s, condition.clone())) {
        return Ok(st);
    }

    // 0aa00. Static damage redirection
    {
        let ecp = ts.checkpoint();
        if let Ok((rep, _)) = try_parse(
            ts,
            crate::v2::clause::player_actions::parse_damage_redirect_replacement,
        ) {
            return Ok(match condition {
                Some(c) => StaticAbility::ConditionalReplacement {
                    condition: c,
                    replacement: Box::new(rep),
                },
                None => StaticAbility::ConditionalReplacement {
                    condition: ConditionExpr::Flag(ConditionFlag::ControlsSelf),
                    replacement: Box::new(rep),
                },
            });
        }
        ts.restore(ecp);
    }

    // 0aa00. Bare Level Up base P/T row (`3/3`, `6/6`)
    if let Some((p, t)) = ts.next_base_pt() {
        return Ok(StaticAbility::LevelStructure(LevelMarker::BasePT(p, t)));
    }

    // 0aa0. Class level header (`Level 2`) or Level Up band (`LEVEL 1-2`, `LEVEL 3+`)
    if ts.eat_word("level")
        && let Some(lvl) = ts.next_number(false).and_then(|n| u8::try_from(n).ok())
    {
        if matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Plus)
        ) {
            ts.next_tok();
            return Ok(StaticAbility::LevelStructure(LevelMarker::Band {
                min: lvl,
                max: None,
            }));
        }
        if matches!(
            ts.peek().map(|t| &t.kind),
            Some(crate::v2::lexer::TokenKind::Minus)
        ) {
            ts.next_tok();
            if let Some(hi) = ts.next_number(false).and_then(|n| u8::try_from(n).ok()) {
                return Ok(StaticAbility::LevelStructure(LevelMarker::Band {
                    min: lvl,
                    max: Some(hi),
                }));
            }
        }
        return Ok(StaticAbility::LevelStructure(LevelMarker::ClassLevel(lvl)));
    }

    // 0aa0b. `If you cast a spell this way, [you] [may] ...`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["if", "you", "cast", "a", "spell", "this", "way"]) && ts.eat_comma() {
            if ts.eat_phrase(&[
                "you", "may", "cast", "it", "as", "though", "it", "had", "flash",
            ]) {
                return Ok(StaticAbility::ZoneCastPermission {
                    player: PlayerRef::You,
                    filter: None,
                    subject: None,
                    zone: None,
                    mode: CastPermissionMode::AsThoughFlash { extra_cost: None },
                    condition,
                });
            }
            let _ = ts.eat_word("you");
            let _ = ts.eat_word("may");
            if let Ok(costs) = try_parse(ts, parse_cost_list)
                && (ts.eat_phrase(&["rather", "than", "pay"])
                    || ts.eat_phrase(&["rather", "than", "paying"]))
                && (ts.eat_phrase(&["its", "mana", "cost"])
                    || ts.eat_phrase(&["that", "spell's", "mana", "cost"])
                    || ts.eat_phrase(&["this", "spell's", "mana", "cost"])
                    || (ts.eat_tilde() && ts.eat_phrase(&["'s", "mana", "cost"])))
            {
                return Ok(StaticAbility::AlternativeCost(costs));
            }
        }
        ts.restore(acp);
    }

    // 0aa0c. `bands with other <flt>` (quoted granted banding ability)
    {
        let bcp = ts.checkpoint();
        if ts.eat_phrase(&["bands", "with", "other"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(StaticAbility::GrantKeywords {
                subject: SubjectRef::SelfObject,
                keywords: vec![crate::v2::ir::KeywordSpec {
                    kind: crate::v2::ir::KeywordKind::Banding,
                    param: Some(crate::v2::ir::KeywordParam::Filter(Box::new(flt))),
                }],
                condition,
            });
        }
        ts.restore(bcp);
    }

    // 0aa0. `If you cast it any time a sorcery couldn't have been cast, the controller of the permanent it becomes sacrifices it at the beginning of the next cleanup step.`
    {
        let fscp = ts.checkpoint();
        if ts.eat_phrase(&["if", "you", "cast", "it", "any", "time"])
            && ts.eat_phrase(&["a", "sorcery", "couldn't", "have", "been", "cast"])
            && ts.eat_comma()
            && ts.eat_phrase(&[
                "the",
                "controller",
                "of",
                "the",
                "permanent",
                "it",
                "becomes",
            ])
            && ts.eat_phrase(&["sacrifices", "it", "at", "the", "beginning"])
            && ts.eat_phrase(&["of", "the", "next", "cleanup", "step"])
        {
            return Ok(StaticAbility::GlobalFlag(
                StaticFlagKind::FlashSacrificeAtCleanup,
            ));
        }
        ts.restore(fscp);
    }

    // 0aa1. Deckbuilding / Reveal-top / Max-hand / Once-per-turn statics
    if ts.eat_phrase(&[
        "play", "with", "the", "top", "card", "of", "your", "library", "revealed",
    ]) {
        return Ok(StaticAbility::VisibilityRule {
            player: PlayerRef::You,
            kind: VisibilityKind::PlayWithTopRevealed,
        });
    }
    if ts.eat_phrase(&["play", "with", "your", "hand", "revealed"]) {
        return Ok(StaticAbility::PlayerRestriction {
            player: PlayerRef::You,
            restriction: PlayerRestrictionKind::PlaysWithHandRevealed,
            condition,
        });
    }
    if ts.eat_phrase(&[
        "a", "deck", "can", "have", "any", "number", "of", "cards", "named",
    ]) && ts.eat_tilde()
    {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AnyNumberInDeck));
    }
    if ts.eat_word("draft") && ts.eat_tilde() && ts.eat_phrase(&["face", "up"]) {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::DraftFaceUp));
    }
    {
        let hcp = ts.checkpoint();
        let hand_player = if ts.eat_phrase(&["your", "maximum", "hand", "size"]) {
            Some(PlayerRef::You)
        } else if ts.eat_phrase(&["each", "opponent's", "maximum", "hand", "size"]) {
            Some(PlayerRef::EachOpponent)
        } else if ts.eat_phrase(&["each", "player's", "maximum", "hand", "size"]) {
            Some(PlayerRef::EachPlayer)
        } else if ts.eat_phrase(&["the", "chosen", "player's", "maximum", "hand", "size"]) {
            Some(PlayerRef::ChosenPlayer)
        } else {
            None
        };
        if let Some(player) = hand_player {
            if ts.eat_phrase(&["is", "equal", "to"]) {
                let value = parse_static_value_expr(ts)?;
                return Ok(StaticAbility::HandSize {
                    player,
                    rule: HandSizeRule::Set(value),
                });
            }
            if ts.eat_phrase(&["is", "increased", "by"])
                && let Some(n) = ts.next_number(false)
            {
                return Ok(StaticAbility::HandSize {
                    player,
                    rule: HandSizeRule::Delta(i16::try_from(n).unwrap_or(1)),
                });
            }
            if ts.eat_phrase(&["is", "reduced", "by"])
                && let Some(n) = ts.next_number(false)
            {
                return Ok(StaticAbility::HandSize {
                    player,
                    rule: HandSizeRule::Delta(-i16::try_from(n).unwrap_or(1)),
                });
            }
            if ts.eat_word("is") {
                let value = parse_value_expr(ts)?;
                return Ok(StaticAbility::HandSize {
                    player,
                    rule: HandSizeRule::Set(value),
                });
            }
        }
        ts.restore(hcp);
    }
    if ts.eat_phrase(&["players", "have", "no", "maximum", "hand", "size"])
        || ts.eat_phrase(&["each", "player", "has", "no", "maximum", "hand", "size"])
    {
        return Ok(StaticAbility::PlayerRestriction {
            player: PlayerRef::EachPlayer,
            restriction: PlayerRestrictionKind::NoMaxHandSize,
            condition,
        });
    }
    // `No more than <N> creature(s) can attack [you/~] each combat`
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["no", "more", "than"])
            && let Some(n) = ts.next_number(false)
            && (ts.eat_word("creature") || ts.eat_word("creatures"))
            && ts.eat_phrase(&["can", "attack"])
        {
            let target = if ts.peek_phrase(&["each", "combat"]) {
                None
            } else {
                opt(ts, parse_subject_ref)
            };
            if ts.eat_phrase(&["each", "combat"]) {
                return Ok(StaticAbility::MaxAttackersPerCombat {
                    max: u8::try_from(n.max(0)).unwrap_or(1),
                    target,
                });
            }
        }
        ts.restore(ncp);
    }
    // `Untap ~ during each other player's untap step`
    {
        let ucp = ts.checkpoint();
        if ts.eat_word("untap")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["during", "each", "other", "player's", "untap", "step"])
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::UntapDuringEachOtherUntap,
                condition,
            });
        }
        ts.restore(ucp);
    }
    // `If <flt | ~> attacks, ~ also attacks if able` OR `If <flt | ~> attacks, all <flt2> attack if able`
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("if") {
            let trigger_flt = if ts.eat_tilde() {
                Ok(ObjectFilter::default())
            } else {
                try_parse(ts, parse_object_filter)
            };
            if let Ok(trigger_filter) = trigger_flt
                && (ts.eat_word("attacks") || ts.eat_word("attack"))
                && ts.eat_comma()
            {
                if ts.eat_tilde() && ts.eat_phrase(&["also", "attacks", "if", "able"]) {
                    return Ok(StaticAbility::FilterRule {
                        filter: trigger_filter,
                        kind: FilterStaticKind::AlsoAttacksIfAble,
                    });
                }
                if ts.eat_word("all")
                    && let Ok(must_flt) = try_parse(ts, parse_object_filter)
                    && (ts.eat_phrase(&["attack", "if", "able"])
                        || ts.eat_phrase(&["attacks", "if", "able"]))
                {
                    return Ok(StaticAbility::SubjectRule {
                        subject: SubjectRef::All(Box::new(must_flt)),
                        rule: SubjectStaticRule::MustAttack,
                        condition,
                    });
                }
            }
        }
        ts.restore(ecp);
    }
    // `You don't lose the game for having 0 or less life`
    {
        let zcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "don't", "lose", "the", "game", "for", "having"])
            && ts.next_number(false) == Some(0)
            && ts.eat_phrase(&["or", "less", "life"])
        {
            return Ok(StaticAbility::GlobalFlag(
                StaticFlagKind::DontLoseForZeroLife,
            ));
        }
        ts.restore(zcp);
    }
    // `Cast ~ only if <cond>`
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("cast")
            && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
            && ts.eat_phrase(&["only", "if"])
            && let Ok(cond) = try_parse(ts, parse_static_condition)
        {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::SelfObject),
                rule: RuleRestrictionKind::CantBeCast,
                condition: Some(and_cond(condition, ConditionExpr::Not(Box::new(cond)))),
            });
        }
        ts.restore(ccp);
    }
    // `<flt1> [and <flt2>] entering [the battlefield] don't cause abilities [of <flt3>] to trigger`
    {
        let dcp = ts.checkpoint();
        if let Ok(mut filter) = try_parse(ts, parse_object_filter) {
            let acp = ts.checkpoint();
            if ts.eat_word("and")
                && let Ok(f2) = try_parse(ts, parse_object_filter)
            {
                filter = ObjectFilter {
                    predicates: vec![FilterPredicate::AnyOf(vec![filter, f2])],
                    ..Default::default()
                };
            } else {
                ts.restore(acp);
            }
            if ts.eat_word("entering") {
                let _ = ts.eat_phrase(&["the", "battlefield"]);
                if ts.eat_phrase(&["don't", "cause", "abilities"]) {
                    let permanent_filter = if ts.eat_word("of") {
                        opt(ts, parse_object_filter)
                    } else {
                        None
                    };
                    if ts.eat_phrase(&["to", "trigger"]) {
                        return Ok(StaticAbility::TriggerModifier {
                            kind: TriggerModifierKind::EntersDontTrigger,
                            filter,
                            permanent_filter,
                            extra_count: 0,
                        });
                    }
                }
            }
        }
        ts.restore(dcp);
    }
    // `Double all damage that <flt> would deal`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["double", "all", "damage", "that"])
            && let Ok(source) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["would", "deal"])
        {
            return Ok(StaticAbility::FilterRule {
                filter: source,
                kind: FilterStaticKind::DoubleDamage,
            });
        }
        ts.restore(dcp);
    }
    // Activation cost modifiers (only forms whose qualifiers fit `ActivationCostModifier`):
    // - `This cost is reduced by {M} [for each ...]`
    // - `Activated abilities cost {M} less/more to activate [for each ...] [if <cond>]`
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["this", "cost", "is", "reduced", "by"])
            && let Some(delta) = ts.eat_mana_cost()
        {
            let scale = opt(ts, parse_static_scale_expr);
            return Ok(StaticAbility::ActivationCostModifier {
                delta,
                less: true,
                scale,
                condition,
            });
        }
        ts.restore(ecp);
        if ts.eat_phrase(&["activated", "abilities"])
            && (ts.eat_word("cost") || ts.eat_word("costs"))
            && let Some(delta) = ts.eat_mana_cost()
        {
            let less = if ts.eat_phrase(&["less", "to", "activate"]) {
                Some(true)
            } else if ts.eat_phrase(&["more", "to", "activate"]) {
                Some(false)
            } else {
                None
            };
            if let Some(less) = less {
                let scale = opt(ts, parse_static_scale_expr);
                let extra_cond = if ts.eat_word("if") {
                    if let Ok(c) = try_parse(ts, parse_static_condition) {
                        Some(c)
                    } else {
                        ts.restore(ecp);
                        return Err(
                            ts.err("expected condition after `if` in activation cost modifier")
                        );
                    }
                } else {
                    None
                };
                return Ok(StaticAbility::ActivationCostModifier {
                    delta,
                    less,
                    scale,
                    condition: merge_cond(condition, extra_cond),
                });
            }
        }
        ts.restore(ecp);
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["all", "creatures", "able", "to", "block"])
            && let Ok(mut subject) = try_parse(ts, parse_subject_ref)
        {
            let ocp = ts.checkpoint();
            if ts.eat_word("or")
                && let Ok(other) = try_parse(ts, parse_subject_ref)
            {
                subject = SubjectRef::Compound(vec![subject, other]);
            } else {
                ts.restore(ocp);
            }
            if ts.eat_phrase(&["do", "so"]) {
                return Ok(StaticAbility::SubjectRule {
                    subject,
                    rule: SubjectStaticRule::AllMustBlock,
                    condition,
                });
            }
        }
        ts.restore(acp);
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "first", "spell", "each", "player", "casts"])
            && ts.eat_phrase(&["during", "each", "of", "their", "turns"])
            && ts.eat_phrase(&[
                "may", "be", "cast", "without", "paying", "its", "mana", "cost",
            ])
        {
            return Ok(StaticAbility::ZoneCastPermission {
                player: PlayerRef::EachPlayer,
                filter: None,
                subject: None,
                zone: None,
                mode: CastPermissionMode::FirstSpellFree,
                condition,
            });
        }
        ts.restore(cp);
    }
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["once", "during", "each", "of", "your", "turns"])
            || ts.eat_phrase(&["once", "each", "turn"]))
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may"])
        {
            if ts.eat_word("pay")
                && let Some(mc) = ts.eat_mana_cost()
                && ts.eat_phrase(&["rather", "than", "pay", "the", "mana", "cost", "for"])
                && let Ok(mut filter) = try_parse(ts, parse_object_filter)
            {
                let _ = ts.eat_phrase(&["you", "cast"]);
                let zone = if ts.eat_word("from") {
                    opt(ts, parse_zone_ref)
                } else {
                    filter.zones.pop()
                };
                let free = mc.symbols.len() == 1
                    && matches!(mc.symbols[0], crate::v2::ir::ManaSymbol::Generic(0));
                return Ok(StaticAbility::ZoneCastPermission {
                    player: PlayerRef::You,
                    filter: Some(filter),
                    subject: None,
                    zone,
                    mode: CastPermissionMode::OncePerTurn {
                        free,
                        alt_cost: if free {
                            None
                        } else {
                            Some(vec![crate::v2::ir::Cost::Mana(mc)])
                        },
                    },
                    condition,
                });
            }
            if (ts.eat_word("cast") || ts.eat_word("play"))
                && let Ok(mut filter) = try_parse(ts, parse_object_filter)
            {
                let mut zone = filter.zones.pop();
                if (ts.eat_phrase(&["or", "cast"]) || ts.eat_phrase(&["and", "cast"]))
                    && let Ok(mut f2) = try_parse(ts, parse_object_filter)
                {
                    if zone.is_none() {
                        zone = f2.zones.pop();
                    }
                    filter = ObjectFilter {
                        predicates: vec![FilterPredicate::AnyOf(vec![filter, f2])],
                        ..Default::default()
                    };
                }
                if zone.is_none() && ts.eat_word("from") {
                    if ts.eat_word("among") {
                        let _ = ts.eat_word("the");
                        if let Ok(mut among_flt) = try_parse(ts, parse_object_filter) {
                            zone = among_flt.zones.pop().or(Some(crate::v2::ir::ZoneRef {
                                zone: crate::v2::ir::ZoneKind::Exile,
                                owner: crate::v2::ir::ZoneOwner::Default,
                            }));
                        }
                    } else {
                        let _ = ts.eat_phrase(&["the", "top", "of"]);
                        zone = opt(ts, parse_zone_ref);
                    }
                }
                let _ = ts.eat_phrase(&["this", "turn"]);
                let free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                    || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
                let alt_cost = if !free && ts.eat_word("by") {
                    let costs = opt(ts, parse_cost_list);
                    let _ = ts.eat_in_addition_to_other_costs()
                        || ts.eat_any_phrase(&[
                            &["rather", "than", "paying", "its", "mana", "cost"],
                            &["rather", "than", "paying", "their", "mana", "costs"],
                        ]);
                    costs
                } else {
                    None
                };
                let _ = ts.eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"]);
                let extra_cond = if ts.eat_word("if") {
                    opt(ts, parse_static_condition)
                } else {
                    None
                };
                return Ok(StaticAbility::ZoneCastPermission {
                    player: PlayerRef::You,
                    filter: Some(filter),
                    subject: None,
                    zone,
                    mode: CastPermissionMode::OncePerTurn { free, alt_cost },
                    condition: merge_cond(condition, extra_cond),
                });
            }
        }
        ts.restore(cp);
    }

    // 0aa. `You control enchanted creature/permanent/enchantment/artifact/land/equipment/planeswalker`
    if ts.eat_phrase(&["you", "control", "enchanted"])
        && (ts.eat_phrase(&["artifact", "creature"])
            || ts.eat_any_word(&[
                "creature",
                "permanent",
                "artifact",
                "enchantment",
                "land",
                "equipment",
                "planeswalker",
            ]))
    {
        return Ok(StaticAbility::ControlEnchanted {
            controller: PlayerRef::You,
        });
    }
    // `Enchanted <type>'s activated abilities can't be activated`
    // (Fails closed if followed by `unless they're mana abilities`, which `ActivatedAbilitiesCantBeActivated` cannot store.)
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("enchanted")
            && let Some(noun) = ts
                .peek_word()
                .and_then(|w| match w.to_ascii_lowercase().as_str() {
                    "creature's" => Some(crate::v2::ir::AnaphoraRef::EnchantedCreature),
                    "permanent's" | "artifact's" | "land's" => {
                        Some(crate::v2::ir::AnaphoraRef::EnchantedPermanent)
                    }
                    _ => None,
                })
        {
            ts.next_word();
            if ts.eat_phrase(&["activated", "abilities", "can't", "be", "activated"]) {
                return Ok(StaticAbility::RuleRestriction {
                    subject: Some(SubjectRef::Anaphora(noun)),
                    rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                    condition,
                });
            }
        }
        ts.restore(ecp);
    }
    // `Mana of any type can be spent to cast <flt>` (only when `filter` is parsed and stored losslessly)
    {
        let mcp = ts.checkpoint();
        if condition.is_none()
            && ts.eat_phrase(&[
                "mana", "of", "any", "type", "can", "be", "spent", "to", "cast",
            ])
            && let Ok(filter) = try_parse(ts, parse_object_filter)
        {
            return Ok(StaticAbility::FilterRule {
                filter,
                kind: FilterStaticKind::SpendManaAsAnyColor,
            });
        }
        ts.restore(mcp);
    }
    // 0ab. Attachment retention and cost-reduction floor rules
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["this", "effect", "doesn't", "remove"])
            && (ts.eat_tilde()
                || (ts.eat_phrase(&["auras", "and", "equipment", "you", "control"])
                    && ts.eat_phrase(&["that", "are", "already", "attached", "to", "it"]))
                || ts.eat_phrase(&["auras", "that", "are", "already", "attached", "to", "it"])
                || (ts.eat_phrase(&["auras", "already", "attached", "to"])
                    && opt(ts, parse_subject_ref).is_some())
                || ts.eat_word("auras"))
        {
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::RetainAttachment));
        }
        ts.restore(rcp);
    }
    {
        let rcp = ts.checkpoint();
        if (ts.eat_phrase(&["this", "effect", "reduces", "only", "the", "amount"])
            && ts.eat_phrase(&["of", "colored", "mana", "you", "pay"]))
            || (ts.eat_phrase(&["this", "effect", "can't", "reduce", "the", "mana", "in"])
                && (ts.eat_phrase(&["that", "cost", "to", "less", "than", "one", "mana"])
                    || (ts.eat_phrase(&["that", "ability's", "activation", "cost"])
                        && ts.eat_phrase(&["to", "less", "than", "one", "mana"]))))
        {
            return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CostReductionRule));
        }
        ts.restore(rcp);
    }
    // `Those spells cost {M} less/more to cast if you paid life this way.`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["those", "spells", "cost"])
            && let Some(delta) = ts.eat_mana_cost()
        {
            let less = if ts.eat_phrase(&["less", "to", "cast"]) {
                Some(true)
            } else if ts.eat_phrase(&["more", "to", "cast"]) {
                Some(false)
            } else {
                None
            };
            if let Some(less) = less
                && ts.eat_phrase(&["if", "you", "paid", "life", "this", "way"])
            {
                return Ok(StaticAbility::CostModifier {
                    subject: ObjectFilter {
                        any_spell: true,
                        ..Default::default()
                    },
                    delta,
                    less,
                    scale: None,
                    min_mana: None,
                    condition: Some(and_cond(
                        condition,
                        ConditionExpr::EventCheck(crate::v2::ir::EventCheckKind::PriorObjectEvent(
                            crate::v2::ir::PriorActionKind::LifePaidAsEntered,
                        )),
                    )),
                });
            }
        }
        ts.restore(dcp);
    }
    {
        let pcan_cp = ts.checkpoint();
        let p_opt = if ts.eat_phrase(&["your", "opponents", "can't"])
            || ts.eat_phrase(&["each", "opponent", "can't"])
            || ts.eat_phrase(&["opponents", "can't"])
        {
            Some(PlayerRef::EachOpponent)
        } else if ts.eat_phrase(&["players", "can't"])
            || ts.eat_phrase(&["each", "player", "can't"])
        {
            Some(PlayerRef::EachPlayer)
        } else if ts.eat_phrase(&["you", "can't"]) {
            Some(PlayerRef::You)
        } else if ts.eat_phrase(&["enchanted", "player", "can't"]) {
            Some(PlayerRef::EnchantedPlayer)
        } else if ts.eat_phrase(&["that", "player", "can't"]) {
            Some(PlayerRef::ThatPlayer)
        } else {
            None
        };
        if let Some(player) = p_opt {
            const SIMPLE_PLAYER_RESTRICTIONS: &[(&[&str], PlayerRestrictionKind)] = &[
                (&["gain", "life"], PlayerRestrictionKind::CantGainLife),
                (
                    &["search", "libraries"],
                    PlayerRestrictionKind::CantSearchLibraries,
                ),
                (
                    &["draw", "more", "than", "one", "card", "each", "turn"],
                    PlayerRestrictionKind::CantDrawMoreThanOne,
                ),
            ];
            for &(phrase, restriction) in SIMPLE_PLAYER_RESTRICTIONS {
                if ts.eat_phrase(phrase) {
                    return Ok(StaticAbility::PlayerRestriction {
                        player,
                        restriction,
                        condition,
                    });
                }
            }
            {
                let cm_cp = ts.checkpoint();
                if ts.eat_phrase(&["cast", "more", "than", "one", "spell", "each", "turn"]) {
                    return Ok(StaticAbility::PlayerRestriction {
                        player,
                        restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
                        condition,
                    });
                }
                ts.restore(cm_cp);
            }
            // `<player> can't untap more than <N> permanent(s) during their/your untap step(s)`
            // (`CantUntapMoreThan(u8)` has no `ObjectFilter`, so filtered types like `land` or `creature` fail closed.)
            {
                let u_cp = ts.checkpoint();
                if ts.eat_phrase(&["untap", "more", "than"])
                    && let Some(n) = ts.next_number(false)
                    && let Ok(max) = u8::try_from(n)
                    && ts.eat_any_word(&["permanent", "permanents"])
                    && ts.eat_any_phrase(&[
                        &["during", "their", "untap", "steps"],
                        &["during", "their", "untap", "step"],
                        &["during", "your", "untap", "step"],
                    ])
                {
                    return Ok(StaticAbility::PlayerRestriction {
                        player,
                        restriction: PlayerRestrictionKind::CantUntapMoreThan(max),
                        condition,
                    });
                }
                ts.restore(u_cp);
            }
            // `<player> can't activate abilities` (fails closed if `that aren't mana abilities` follows)
            if ts.eat_phrase(&["activate", "abilities"]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantActivateAbilities,
                    condition,
                });
            }
            // `<player> can't play lands [from <zone>] [if <cond>]` / `<player> can't play <flt> [from <zone>]`
            {
                let pl_cp = ts.checkpoint();
                if ts.eat_word("play")
                    && !ts.peek_phrase(&["lands", "or", "cast"])
                    && let Ok(mut filter) = try_parse(ts, parse_object_filter)
                {
                    if ts.eat_word("from")
                        && let Ok(z) = try_parse(ts, parse_zone_ref)
                    {
                        filter.zones.push(z);
                    }
                    let extra_cond = if ts.eat_word("if") || ts.eat_phrase(&["as", "long", "as"]) {
                        opt(ts, parse_static_condition)
                    } else {
                        None
                    };
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition: merge_cond(condition, extra_cond),
                    });
                }
                ts.restore(pl_cp);
            }
            if ts.eat_phrase(&["cast", "spells"]) {
                let pred = if ts.eat_phrase(&["with", "the", "chosen", "name"]) {
                    Some(FilterPredicate::OfChosenName)
                } else if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
                    Some(FilterPredicate::OfChosenColor)
                } else if ts.eat_phrase(&["of", "the", "chosen", "type"]) {
                    Some(FilterPredicate::OfChosenCreatureType)
                } else if ts.eat_any_phrase(&[
                    &["from", "anywhere", "other", "than", "their", "hands"],
                    &["from", "anywhere", "other", "than", "your", "hand"],
                ]) {
                    Some(FilterPredicate::FromOutsideHand)
                } else if ts.eat_phrase(&["with", "the", "same", "name", "as"])
                    && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
                {
                    Some(FilterPredicate::SharesPropertyWith {
                        property: crate::v2::ir::SharedProperty::Name,
                        reference: Box::new(ref_subj),
                        negated: false,
                    })
                } else {
                    None
                };
                if let Some(p) = pred {
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter: ObjectFilter {
                            any_spell: true,
                            predicates: vec![p],
                            ..Default::default()
                        },
                        condition,
                    });
                }
                {
                    let mv_cp = ts.checkpoint();
                    if ts.eat_phrase(&["with", "mana", "value"])
                        && let Ok(val) = try_parse(ts, parse_value_expr)
                    {
                        let op = if ts.eat_phrase(&["or", "less"]) {
                            crate::ast::CompareOp::Le
                        } else if ts.eat_phrase(&["or", "greater"]) {
                            crate::ast::CompareOp::Ge
                        } else {
                            crate::ast::CompareOp::Eq
                        };
                        let mut filter = ObjectFilter {
                            any_spell: true,
                            ..Default::default()
                        };
                        filter.stat_bounds.push(crate::v2::ir::StatBound {
                            stat: crate::ast::StatKind::ManaValue,
                            op,
                            value: Box::new(val),
                        });
                        return Ok(StaticAbility::CantCastSpellsMatching {
                            player,
                            filter,
                            condition,
                        });
                    }
                    ts.restore(mv_cp);
                }
                if ts.eat_phrase(&["or", "activate", "abilities"]) {
                    let first = StaticAbility::PlayerRestriction {
                        player,
                        restriction: PlayerRestrictionKind::CantCastSpells,
                        condition: condition.clone(),
                    };
                    if ts.eat_word("of")
                        && let Ok(flt) = try_parse(ts, parse_object_filter)
                    {
                        return Ok(StaticAbility::Compound(vec![
                            first,
                            StaticAbility::RuleRestriction {
                                subject: Some(SubjectRef::All(Box::new(flt))),
                                rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                                condition,
                            },
                        ]));
                    }
                    return Ok(StaticAbility::Compound(vec![
                        first,
                        StaticAbility::PlayerRestriction {
                            player,
                            restriction: PlayerRestrictionKind::CantActivateAbilities,
                            condition,
                        },
                    ]));
                }
                if ts.eat_word("from")
                    && let Ok(z1) = try_parse(ts, parse_zone_ref)
                {
                    let mut filter = ObjectFilter {
                        any_spell: true,
                        zones: vec![z1],
                        ..Default::default()
                    };
                    while ts.eat_word("or") {
                        if let Ok(z_more) = try_parse(ts, parse_zone_ref) {
                            filter.zones.push(z_more);
                        } else if ts.eat_phrase(&["activate", "abilities", "of", "cards", "in"])
                            && let Ok(az) = try_parse(ts, parse_zone_ref)
                        {
                            let first = StaticAbility::CantCastSpellsMatching {
                                player,
                                filter,
                                condition: condition.clone(),
                            };
                            let act_flt = ObjectFilter {
                                any_card: true,
                                zones: vec![az],
                                ..Default::default()
                            };
                            return Ok(StaticAbility::Compound(vec![
                                first,
                                StaticAbility::RuleRestriction {
                                    subject: Some(SubjectRef::All(Box::new(act_flt))),
                                    rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                                    condition,
                                },
                            ]));
                        } else {
                            break;
                        }
                    }
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition,
                    });
                }
                if let Some(filter) = opt(ts, parse_object_filter) {
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition,
                    });
                }
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantCastSpells,
                    condition,
                });
            }
            // `<player> can't cast ~ [if/unless <cond>]`
            {
                let sc_cp = ts.checkpoint();
                if ts.eat_word("cast") && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"])) {
                    let extra_cond = if ts.eat_word("if") {
                        opt(ts, parse_static_condition)
                    } else if ts.eat_word("unless") {
                        opt(ts, parse_static_condition).map(|c| ConditionExpr::Not(Box::new(c)))
                    } else {
                        None
                    };
                    if let Some(c) = extra_cond {
                        return Ok(StaticAbility::RuleRestriction {
                            subject: Some(SubjectRef::SelfObject),
                            rule: RuleRestrictionKind::CantBeCast,
                            condition: Some(and_cond(condition, c)),
                        });
                    }
                }
                ts.restore(sc_cp);
            }
            // `<player> can't cast <flt> [from <zone> [or <zone2>]]`
            {
                let ccp = ts.checkpoint();
                if ts.eat_word("cast")
                    && let Ok(mut filter) = try_parse(ts, parse_object_filter)
                {
                    if ts.eat_word("from")
                        && let Ok(z1) = try_parse(ts, parse_zone_ref)
                    {
                        filter.zones.push(z1);
                        while ts.eat_word("or") {
                            if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                                filter.zones.push(z2);
                            } else {
                                break;
                            }
                        }
                    }
                    if ts.eat_phrase(&["or", "activate", "abilities"]) {
                        return Ok(StaticAbility::Compound(vec![
                            StaticAbility::CantCastSpellsMatching {
                                player,
                                filter,
                                condition: condition.clone(),
                            },
                            StaticAbility::PlayerRestriction {
                                player,
                                restriction: PlayerRestrictionKind::CantActivateAbilities,
                                condition,
                            },
                        ]));
                    }
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition,
                    });
                }
                ts.restore(ccp);
            }
            // `<player> can't play lands or cast spells from your/their hand`
            if ts.eat_phrase(&[
                "play", "lands", "or", "cast", "spells", "from", "your", "hand",
            ]) || ts.eat_phrase(&[
                "play", "lands", "or", "cast", "spells", "from", "their", "hand",
            ]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantPlayFromHand,
                    condition,
                });
            }
            // `<player> can't win the game / lose the game [and <p2> can't ...]`
            {
                let p_restr = if ts.eat_phrase(&["win", "the", "game"]) {
                    Some(PlayerRestrictionKind::CantWinGame)
                } else if ts.eat_phrase(&["lose", "the", "game"]) {
                    Some(PlayerRestrictionKind::CantLoseGame)
                } else {
                    None
                };
                if let Some(restriction) = p_restr {
                    let first = StaticAbility::PlayerRestriction {
                        player,
                        restriction,
                        condition: condition.clone(),
                    };
                    let acp = ts.checkpoint();
                    if ts.eat_word("and")
                        && let Ok(second) =
                            try_parse(ts, |s| parse_static_core(s, condition.clone()))
                    {
                        return Ok(StaticAbility::Compound(vec![first, second]));
                    }
                    ts.restore(acp);
                    return Ok(first);
                }
            }
            ts.restore(pcan_cp);
        }
    }

    // 0ac. `Prevent all [combat] damage [that would be dealt [to <subj>] [by <subj>] | [that] <subj> would deal [to <subj>]]`
    if ts.eat_phrase(&["prevent", "all"]) {
        let combat_only = ts.eat_word("combat");
        if ts.eat_word("damage") {
            let mut source = None;
            let mut target = None;
            if ts.eat_phrase(&["that", "would", "be", "dealt"]) {
                loop {
                    if ts.eat_phrase(&["to", "and", "dealt", "by"]) {
                        let subj = parse_subject_ref(ts)?;
                        target = Some(subj.clone());
                        source = Some(subj);
                        continue;
                    }
                    if ts.eat_word("to") {
                        target = Some(parse_subject_ref(ts)?);
                        continue;
                    }
                    if ts.eat_word("by") {
                        source = Some(parse_subject_ref(ts)?);
                        continue;
                    }
                    break;
                }
            } else {
                let acp = ts.checkpoint();
                let _ = ts.eat_word("that");
                if let Ok(src) = try_parse(ts, parse_subject_ref)
                    && ts.eat_phrase(&["would", "deal"])
                {
                    source = Some(src);
                    if ts.eat_word("to") {
                        target = opt(ts, parse_subject_ref);
                    }
                } else {
                    ts.restore(acp);
                }
            }
            return Ok(StaticAbility::PreventDamageStatic {
                combat_only,
                source,
                target,
            });
        }
    }

    // 0ad. `This ability costs {M} less/more to activate [for each <expr>]`
    if ts.eat_phrase(&["this", "ability", "costs"]) {
        let delta = ts
            .eat_mana_cost()
            .ok_or_else(|| ts.err("expected mana delta in activation cost modifier"))?;
        let less = if ts.eat_phrase(&["less", "to", "activate"]) {
            true
        } else if ts.eat_phrase(&["more", "to", "activate"]) {
            false
        } else {
            return Err(ts.err("expected `less/more to activate`"));
        };
        let scale = opt(ts, parse_static_scale_expr);
        return Ok(StaticAbility::ActivationCostModifier {
            delta,
            less,
            scale,
            condition,
        });
    }

    // 0a. `~ can be your commander` / `~ enters prepared` / `~'s power and toughness are each equal to <val>`
    {
        let cp = ts.checkpoint();
        let self_or_its = if ts.eat_tilde() {
            let _ = ts.eat_word("'s");
            true
        } else {
            ts.eat_word("its")
        };
        if self_or_its {
            if ts.eat_phrase(&["can", "be", "your", "commander"]) {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CanBeCommander));
            }
            if ts.eat_phrase(&["is", "all", "colors"]) {
                let _ = ts.eat_period();
                let _ = ts.eat_phrase(&[
                    "this", "ability", "doesn't", "affect", "its", "color", "identity",
                ]);
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AllColors));
            }
            if ts.eat_phrase(&["saddles", "mounts", "and", "crews", "vehicles"])
                && ts.eat_phrase(&["as", "though", "its", "power", "were"])
                && let Some(n) = ts.next_number(false)
                && ts.eat_word("greater")
            {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CrewPowerBonus(n)));
            }
            if ts.eat_phrase(&["enters", "prepared"]) {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::EntersPrepared));
            }
            if ts.eat_phrase(&["power", "and", "toughness", "are", "each", "equal", "to"]) {
                let val = if ts.eat_phrase(&["the", "life", "paid", "as", "it", "entered"]) {
                    ValueExpr::PriorActionCount(crate::v2::ir::PriorActionKind::LifePaidAsEntered)
                } else {
                    parse_static_value_expr(ts)?
                };
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val.clone()),
                    toughness: Some(val),
                });
            }
            if ts.eat_phrase(&["power", "and", "toughness", "are", "each"])
                && let Some(n) = ts.next_number(false)
            {
                return Ok(StaticAbility::SetBasePT {
                    subject: SubjectRef::SelfObject,
                    power: n,
                    toughness: n,
                });
            }
            if ts.eat_phrase(&["base", "power", "is", "equal", "to"]) {
                let val = parse_static_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val),
                    toughness: None,
                });
            }
            if ts.eat_phrase(&["power", "is", "equal", "to"]) {
                let val = parse_static_value_expr(ts)?;
                let toughness = if ts.eat_phrase(&["and", "its", "toughness", "is", "equal", "to"])
                {
                    if ts.eat_phrase(&["that", "number", "plus"])
                        && let Some(n) = ts.next_number(false)
                    {
                        Some(ValueExpr::Add(
                            Box::new(val.clone()),
                            Box::new(ValueExpr::Const(n)),
                        ))
                    } else {
                        Some(parse_static_value_expr(ts)?)
                    }
                } else {
                    None
                };
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val),
                    toughness,
                });
            }
            if ts.eat_phrase(&["toughness", "is", "equal", "to"]) {
                let val = parse_static_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: None,
                    toughness: Some(val),
                });
            }
        }
        ts.restore(cp);
    }

    // 0b. `Skip your <step>` / `<player> skip(s) their <step> steps`
    if ts.eat_phrase(&["skip", "your"]) {
        let step = if ts.eat_phrase(&["draw", "step"]) {
            TurnStepAst::DrawStep
        } else if ts.eat_phrase(&["upkeep", "step"]) || ts.eat_word("upkeep") {
            TurnStepAst::Upkeep
        } else if ts.eat_phrase(&["untap", "step"]) {
            TurnStepAst::UntapStep
        } else if ts.eat_phrase(&["end", "step"]) {
            TurnStepAst::EndStep
        } else {
            return Err(ts.err("expected step name after `Skip your`"));
        };
        return Ok(StaticAbility::SkipStep {
            player: PlayerRef::You,
            step,
        });
    }
    {
        let skcp = ts.checkpoint();
        if let Ok(player) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_any_phrase(&[&["skip", "their"], &["skips", "their"]])
        {
            const STEP_PHRASES: &[(&[&str], TurnStepAst)] = &[
                (&["untap", "steps"], TurnStepAst::UntapStep),
                (&["untap", "step"], TurnStepAst::UntapStep),
                (&["upkeep", "steps"], TurnStepAst::Upkeep),
                (&["upkeep", "step"], TurnStepAst::Upkeep),
                (&["draw", "steps"], TurnStepAst::DrawStep),
                (&["draw", "step"], TurnStepAst::DrawStep),
                (&["end", "steps"], TurnStepAst::EndStep),
                (&["end", "step"], TurnStepAst::EndStep),
            ];
            if let Some((_, step)) = STEP_PHRASES.iter().find(|(p, _)| ts.eat_phrase(p)) {
                return Ok(StaticAbility::SkipStep {
                    player,
                    step: *step,
                });
            }
        }
        ts.restore(skcp);
    }

    // 0b. Trigger-doubling static (delegated to `global_rules.rs`)
    if let Ok(tm) = try_parse(ts, global_rules::parse_trigger_doubler) {
        return Ok(tm);
    }

    // 0c. Player-headed static abilities (`player_rules.rs`)
    if let Ok(st) = try_parse(ts, |s| parse_player_static_ability(s, condition.clone())) {
        return Ok(st);
    }

    // 0d. `[Combat] damage can't be prevented`
    if ts.eat_phrase(&["damage", "can't", "be", "prevented"])
        || ts.eat_phrase(&["combat", "damage", "can't", "be", "prevented"])
    {
        return Ok(StaticAbility::RuleRestriction {
            subject: None,
            rule: RuleRestrictionKind::DamageCantBePrevented,
            condition,
        });
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_word("the")
            && ts
                .next_quoted()
                .is_some_and(|q| q.eq_ignore_ascii_case("legend rule"))
            && ts.eat_phrase(&["doesn't", "apply"])
        {
            let filter = if ts.eat_word("to") {
                if ts.eat_word("them") || ts.eat_word("it") {
                    None
                } else {
                    opt(ts, parse_object_filter)
                }
            } else {
                None
            };
            return Ok(StaticAbility::LegendRuleDoesntApply(filter));
        }
        ts.restore(lcp);
    }
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["activated", "abilities", "of", "sources", "with"])
            && ts.eat_phrase(&["the", "chosen", "name", "can't", "be", "activated"])
        {
            return Ok(StaticAbility::GlobalFlag(
                StaticFlagKind::NamedSourcesCantActivate,
            ));
        }
        ts.restore(ncp);
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["activated", "abilities", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["can't", "be", "activated"])
        {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::All(Box::new(flt))),
                rule: RuleRestrictionKind::ActivatedAbilitiesCantBeActivated,
                condition,
            });
        }
        ts.restore(acp);
    }

    // 1. `The next/first <flt> you/they cast this/each turn ...`
    if let Ok(nsm) = try_parse(ts, |s| parse_next_spell_modifier(s, condition.clone())) {
        return Ok(nsm);
    }

    // 1z. `Spells and abilities can't be countered`
    if ts.eat_phrase(&["spells", "and", "abilities", "can't", "be", "countered"]) {
        return Ok(StaticAbility::RuleRestriction {
            subject: None,
            rule: RuleRestrictionKind::CantBeCountered,
            condition,
        });
    }

    // 2. Static spell cost modifier:
    //    `[The first ]<filter> [<caster> cast(s)] [each turn] cost(s) {M} more/less to cast [for each <expr>] [and can't be countered]`
    if let Ok(cm) = try_parse(ts, |s| parse_cost_modifier(s, condition.clone())) {
        if ts.eat_phrase(&["and", "can't", "be", "countered"]) {
            return Ok(StaticAbility::Compound(vec![
                cm,
                StaticAbility::RuleRestriction {
                    subject: None,
                    rule: RuleRestrictionKind::CantBeCountered,
                    condition,
                },
            ]));
        }
        return Ok(cm);
    }

    // 2b. `it's a[n] [P/T] [color] [subtypes] <types> [with base power and toughness P/T | with power and toughness each equal to its mana value] [in addition to its other types] [and can't be blocked] [and [it] has <kws>]`
    {
        let itcp = ts.checkpoint();
        if ts.eat_phrase(&["it's", "a"]) || ts.eat_phrase(&["it's", "an"]) {
            let mut base_pt = ts.next_base_pt();
            let mut colors = crate::ast::ColorMask::EMPTY;
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(c) = crate::vocab::lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else if low == "and" {
                    let acp = ts.checkpoint();
                    ts.next_word();
                    if ts.peek_word().is_some_and(|nw| {
                        let nl = nw.to_ascii_lowercase();
                        crate::vocab::lookup_color(&nl).is_some()
                            || crate::vocab::lookup_card_type(&nl).is_some()
                            || ts.vocab.lookup_subtype(&nl).is_some()
                    }) {
                        continue;
                    }
                    ts.restore(acp);
                    break;
                } else if crate::vocab::lookup_card_type(&low).is_some() {
                    ts.next_word();
                    saw_type = true;
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else {
                    break;
                }
            }
            if saw_type || !subtypes.is_empty() {
                let mut parts = Vec::new();
                let subj = SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::It);
                if base_pt.is_none()
                    && ts.eat_phrase(&["with", "base", "power", "and", "toughness"])
                {
                    base_pt = ts.next_base_pt();
                }
                {
                    let wcp = ts.checkpoint();
                    if ts.eat_word("with")
                        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
                    {
                        parts.push(StaticAbility::GrantKeywords {
                            subject: subj.clone(),
                            keywords,
                            condition: condition.clone(),
                        });
                    } else {
                        ts.restore(wcp);
                    }
                }
                {
                    let mv_cp = ts.checkpoint();
                    if ts.eat_phrase(&["with", "power", "and", "toughness"])
                        && ts.eat_phrase(&["each", "equal", "to", "its", "mana", "value"])
                    {
                        let mv = ValueExpr::ObjectStat {
                            subject: Box::new(subj.clone()),
                            stat: crate::ast::StatKind::ManaValue,
                        };
                        parts.push(StaticAbility::DynamicBasePT {
                            power: Some(mv.clone()),
                            toughness: Some(mv),
                        });
                    } else {
                        ts.restore(mv_cp);
                    }
                }
                let keep_existing = ts.eat_in_addition_to_other_types();
                if !colors.is_empty() {
                    parts.push(StaticAbility::SubjectRule {
                        subject: subj.clone(),
                        rule: SubjectStaticRule::SetColors {
                            colors,
                            keep_existing,
                        },
                        condition: condition.clone(),
                    });
                }
                if let Some((power, toughness)) = base_pt {
                    parts.push(StaticAbility::SetBasePT {
                        subject: subj.clone(),
                        power,
                        toughness,
                    });
                }
                parts.push(StaticAbility::SetSubtypes {
                    subject: subj.clone(),
                    subtypes,
                    keep_existing,
                });
                loop {
                    let t2 = ts.checkpoint();
                    let _ = ts.eat_comma();
                    if ts.eat_phrase(&["and", "can't"])
                        && let Ok(rule) = try_parse(ts, subject_rules_ext::parse_cant_rule)
                    {
                        parts.push(StaticAbility::RuleRestriction {
                            subject: Some(subj.clone()),
                            rule,
                            condition: condition.clone(),
                        });
                        continue;
                    }
                    ts.restore(t2);
                    let _ = ts.eat_comma();
                    if ts.eat_any_phrase(&[&["and", "it", "has"], &["and", "has"]])
                        && let Ok((keywords, _)) = try_parse(ts, parse_keyword_list)
                    {
                        parts.push(StaticAbility::GrantKeywords {
                            subject: subj.clone(),
                            keywords,
                            condition: condition.clone(),
                        });
                        continue;
                    }
                    ts.restore(t2);
                    break;
                }
                return Ok(wrap_compound(parts));
            }
        }
        ts.restore(itcp);
    }

    // 3. Subject-headed static abilities (`<subj> gets/get +X/+Y ...`, `<subj> can't ...`, etc.)
    subject_rules::parse_subject_static_ability(ts, condition)
}
