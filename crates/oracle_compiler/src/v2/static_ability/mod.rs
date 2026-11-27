//! Strictly-typed [`StaticAbility`] parser for `v2`.

use crate::ast::TurnStepAst;
use crate::v2::condition::parse_condition;
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
use crate::v2::value::{parse_for_each_expr, parse_value_expr};

/// Parse one strictly-typed [`StaticAbility`] sentence (fail-closed: zero leftover tokens).
mod cost_modifiers;
mod player_rules;
mod subject_rules;
use cost_modifiers::{parse_cost_modifier, parse_next_spell_modifier};
use player_rules::parse_player_static_ability;

pub fn parse_static_ability(ts: &mut TokenStream<'_>) -> PResult<StaticAbility> {
    // Optional leading (possibly stacked!) `During your turn, `, `As long as <condition>, `, or `If <condition>, `
    let mut condition: Option<ConditionExpr> = None;
    loop {
        let next_cond = if ts.eat_phrase(&["as", "long", "as"]) {
            let c = parse_condition(ts)?;
            if !ts.eat_comma() {
                return Err(ts.err("expected `,` after `As long as <condition>`"));
            }
            Some(c)
        } else if ts.eat_phrase(&["during", "your", "turn"]) && ts.eat_comma() {
            Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn))
        } else if ts.eat_phrase(&["during", "turns", "other", "than", "yours"]) && ts.eat_comma() {
            Some(ConditionExpr::Not(Box::new(ConditionExpr::Flag(
                ConditionFlag::IsYourTurn,
            ))))
        } else {
            let icp = ts.checkpoint();
            if ts.eat_word("if")
                && let Ok(c) = try_parse(ts, parse_condition)
                && ts.eat_comma()
            {
                Some(c)
            } else {
                ts.restore(icp);
                None
            }
        };
        match next_cond {
            Some(c) => {
                condition = Some(match condition {
                    Some(prev) => ConditionExpr::And(Box::new(prev), Box::new(c)),
                    None => c,
                });
            }
            None => break,
        }
    }

    // Strip trailing `as long as <condition>` if present!
    let end_trim = if matches!(
        ts.tokens.last().map(|t| &t.kind),
        Some(crate::v2::lexer::TokenKind::Period)
    ) {
        ts.tokens.len() - 1
    } else {
        ts.tokens.len()
    };

    // Strip trailing `, where X is <ValueExpr>` binder
    if let Some(where_idx) = find_where_x_is(ts.tokens, ts.pos, end_trim) {
        let expr_slice = &ts.tokens[where_idx + 4..end_trim];
        let mut expr_ts = ts.sub_stream(expr_slice, ts.stage);
        if let Ok(expr) = parse_value_expr(&mut expr_ts)
            && expr_ts.is_eof()
        {
            let body_slice = &ts.tokens[ts.pos..where_idx];
            let mut body_ts = ts.sub_stream(body_slice, ts.stage);
            let inner = parse_static_core(&mut body_ts, condition.take())?;
            body_ts.expect_eof()?;
            ts.pos = ts.tokens.len();
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
        let cond_slice = &ts.tokens[ala_idx + 3..end_trim];
        let mut cond_ts = ts.sub_stream(cond_slice, ts.stage);
        if let Ok(c) = parse_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let body_slice = &ts.tokens[ts.pos..ala_idx];
            let mut body_ts = ts.sub_stream(body_slice, ts.stage);
            let st = parse_static_core(&mut body_ts, Some(c))?;
            body_ts.expect_eof()?;
            ts.pos = ts.tokens.len();
            return Ok(st);
        }
    }

    // Strip trailing `during your turn` (`Kutzil`, `Grand Abolisher`) or `during combat` (`Marisi`, `Basandra`)
    if condition.is_none()
        && end_trim >= ts.pos + 3
        && matches!(ts.tokens[end_trim - 3].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("during"))
        && matches!(ts.tokens[end_trim - 2].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("your"))
        && matches!(ts.tokens[end_trim - 1].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("turn"))
    {
        let body_slice = &ts.tokens[ts.pos..end_trim - 3];
        let mut body_ts = ts.sub_stream(body_slice, ts.stage);
        if let Ok(st) = parse_static_core(
            &mut body_ts,
            Some(ConditionExpr::Flag(ConditionFlag::IsYourTurn)),
        ) && body_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(st);
        }
    }
    if condition.is_none()
        && end_trim >= ts.pos + 2
        && matches!(ts.tokens[end_trim - 2].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("during"))
        && matches!(ts.tokens[end_trim - 1].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("combat"))
    {
        let body_slice = &ts.tokens[ts.pos..end_trim - 2];
        let mut body_ts = ts.sub_stream(body_slice, ts.stage);
        if let Ok(st) = parse_static_core(
            &mut body_ts,
            Some(ConditionExpr::EventCheck(EventCheckKind::DuringStep(
                TurnStepAst::BeginningOfCombat,
            ))),
        ) && body_ts.is_eof()
        {
            ts.pos = ts.tokens.len();
            return Ok(st);
        }
    }

    // Strip trailing `if <cond>` on static abilities (`Mindbender Spores`, `Veldt`)
    if condition.is_none()
        && let Some(if_idx) = (ts.pos + 1..end_trim)
            .find(|&idx| matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("if")))
    {
        let cond_slice = &ts.tokens[if_idx + 1..end_trim];
        let mut cond_ts = ts.sub_stream(cond_slice, ts.stage);
        if let Ok(c) = parse_condition(&mut cond_ts)
            && cond_ts.is_eof()
        {
            let body_slice = &ts.tokens[ts.pos..if_idx];
            let mut body_ts = ts.sub_stream(body_slice, ts.stage);
            if let Ok(st) = parse_static_core(&mut body_ts, Some(c))
                && body_ts.is_eof()
            {
                ts.pos = ts.tokens.len();
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

    let mut parts = vec![parse_static_core(ts, condition.take())?];
    // Compositional `, and <second static ability>` (`Varchild`, `Serra Bestiary`, `Katabatic Winds`, `Katilda`, `Rogue Class`, `Null Summoner`)
    let ccp = ts.checkpoint();
    if ts.eat_comma()
        && ts.eat_word("and")
        && let Ok(next) = try_parse(ts, parse_static_ability)
    {
        parts.push(next);
    } else {
        ts.restore(ccp);
    }
    let _ = ts.eat_period();
    ts.expect_eof()?;
    Ok(if parts.len() == 1 {
        parts.remove(0)
    } else {
        StaticAbility::Compound(parts)
    })
}

fn find_where_x_is(
    tokens: &[crate::v2::lexer::Token<'_>],
    start: usize,
    end: usize,
) -> Option<usize> {
    use crate::v2::lexer::TokenKind;
    if end < start + 5 {
        return None;
    }
    for i in start..=end - 5 {
        if matches!(tokens[i].kind, TokenKind::Comma)
            && matches!(tokens[i + 1].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("where"))
            && matches!(tokens[i + 2].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("x"))
            && matches!(tokens[i + 3].kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("is"))
        {
            return Some(i);
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

fn parse_static_core(
    ts: &mut TokenStream<'_>,
    condition: Option<ConditionExpr>,
) -> PResult<StaticAbility> {
    // 0. `As an additional cost to cast this spell/~/<flt>, [you may] <cost>`
    {
        let ac_cp = ts.checkpoint();
        if ts.eat_phrase(&["as", "an", "additional", "cost", "to", "cast"])
            && (ts.eat_phrase(&["this", "spell"])
                || ts.eat_tilde()
                || try_parse(ts, parse_object_filter).is_ok())
            && ts.eat_comma()
        {
            let _ = ts.eat_phrase(&["you", "may"]);
            if let Ok(costs) = try_parse(ts, parse_cost_list) {
                return Ok(StaticAbility::AdditionalCost(costs));
            }
        }
        ts.restore(ac_cp);
    }

    // 0aa00. `The escape/replicate cost is equal to ...` & static damage redirection (`Underworld Breach`, `Djinn Illuminatus`, `Weathered Bodyguards`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["the", "escape", "cost", "is", "equal", "to"])
            && (ts.eat_phrase(&["its", "mana", "cost"])
                || ts.eat_phrase(&["the", "card's", "mana", "cost"]))
        {
            let mut costs = vec![crate::v2::ir::Cost::PayAnyAmountMana];
            if ts.eat_word("plus")
                && let Ok(extra) = try_parse(ts, parse_cost_list)
            {
                costs.extend(extra);
            }
            return Ok(StaticAbility::AlternativeCost(costs));
        }
        ts.restore(ecp);
        if ts.eat_phrase(&[
            "the",
            "replicate",
            "cost",
            "is",
            "equal",
            "to",
            "its",
            "mana",
            "cost",
        ]) {
            return Ok(StaticAbility::AlternativeCost(vec![
                crate::v2::ir::Cost::PayAnyAmountMana,
            ]));
        }
        ts.restore(ecp);
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

    // 0aa0b. `If you cast a spell this way, [you] pay <cost> rather than pay [its | that spell's] mana cost` (`Bolas's Citadel`, `Eye of Duskmantle`)
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["if", "you", "cast", "a", "spell", "this", "way"]) && ts.eat_comma() {
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

    // 0aa0. `If you cast it any time a sorcery couldn't have been cast, the controller of the permanent it becomes sacrifices it at the beginning of the next cleanup step.` (`Spider Climb` flash-Aura cycle)
    if ts.eat_phrase(&[
        "if", "you", "cast", "it", "any", "time", "a", "sorcery", "couldn't", "have", "been",
        "cast",
    ]) && ts.eat_comma()
        && ts.eat_phrase(&[
            "the",
            "controller",
            "of",
            "the",
            "permanent",
            "it",
            "becomes",
            "sacrifices",
            "it",
            "at",
            "the",
            "beginning",
            "of",
            "the",
            "next",
            "cleanup",
            "step",
        ])
    {
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::FlashSacrificeAtCleanup,
        ));
    }

    // 0aa1. Deckbuilding / Conspiracy / Reveal-top / Max-hand / Once-per-turn statics
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
            condition: None,
        });
    }
    if (ts.eat_phrase(&[
        "a", "deck", "can", "have", "any", "number", "of", "cards", "named",
    ]) || (ts.eat_phrase(&["a", "deck", "can", "have", "up", "to"])
        && ts.next_number(false).is_some()
        && ts.eat_phrase(&["cards", "named"])))
        && ts.eat_tilde()
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
        } else {
            None
        };
        if let Some(player) = hand_player {
            if ts.eat_phrase(&["is", "equal", "to"]) {
                let value = parse_value_expr(ts)?;
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
    // `No more than <N> creature(s) can attack [you/~] each combat` (`Silent Arbiter`, `Crawlspace`, `The Eternal Wanderer`)
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
    // `Untap ~ during each other player's untap step` (`Urban Burgeoning`, `Seedborn Muse`)
    {
        let ucp = ts.checkpoint();
        if ts.eat_word("untap")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["during", "each", "other", "player's", "untap", "step"])
        {
            return Ok(StaticAbility::SubjectRule {
                subject,
                rule: SubjectStaticRule::UntapDuringEachOtherUntap,
                condition: None,
            });
        }
        ts.restore(ucp);
    }
    // `If a creature you control attacks, ~ also attacks if able` (`Ekundu Cyclops`)
    {
        let ecp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(trigger_filter) = try_parse(ts, parse_object_filter)
            && (ts.eat_word("attacks") || ts.eat_word("attack"))
            && ts.eat_comma()
            && ts.eat_tilde()
            && ts.eat_phrase(&["also", "attacks", "if", "able"])
        {
            return Ok(StaticAbility::FilterRule {
                filter: trigger_filter,
                kind: FilterStaticKind::AlsoAttacksIfAble,
            });
        }
        ts.restore(ecp);
    }
    // `You don't lose the game for having 0 or less life` (`Lich`)
    if ts.eat_phrase(&[
        "you", "don't", "lose", "the", "game", "for", "having", "0", "or", "less", "life",
    ]) {
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::DontLoseForZeroLife,
        ));
    }
    // `You don't lose unspent <color> mana as steps and phases end` (`Fangorn`)
    {
        let fcp = ts.checkpoint();
        if ts.eat_phrase(&["you", "don't", "lose", "unspent"])
            && let Some(w) = ts.peek_word()
            && let Some(_c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&["mana", "as", "steps", "and", "phases", "end"]) {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CostReductionRule));
            }
        }
        ts.restore(fcp);
    }
    // `Cast ~ only if <cond>` (`Talara's Battalion`)
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("cast")
            && ts.eat_tilde()
            && ts.eat_phrase(&["only", "if"])
            && let Ok(cond) = try_parse(ts, parse_condition)
        {
            return Ok(StaticAbility::RuleRestriction {
                subject: Some(SubjectRef::SelfObject),
                rule: RuleRestrictionKind::CantBeCast,
                condition: Some(ConditionExpr::Not(Box::new(cond))),
            });
        }
        ts.restore(ccp);
    }
    // `<flt> entering don't cause abilities to trigger` (`Doorkeeper Thrull`, `Torpor Orb`)
    {
        let dcp = ts.checkpoint();
        if let Ok(filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["entering", "don't", "cause", "abilities", "to", "trigger"])
        {
            return Ok(StaticAbility::TriggerModifier {
                kind: TriggerModifierKind::EntersDontTrigger,
                filter,
                permanent_filter: None,
                extra_count: 0,
            });
        }
        ts.restore(dcp);
    }
    // `Double all damage that <flt> would deal` (`Raphael`, `Gratuitous Violence`)
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
    // `Equip abilities you activate [that target <subj>] cost {M} less/more to activate` (`Strong Back`)
    // `Abilities [your opponents | players | you] activate [that target <flt>] cost {M} more/less to activate` (`Kopala, Warden of Waves`)
    // `Activated abilities of <flt> cost {M} more/less to activate [this effect can't ...]` (`Training Grounds`, `Heartstone`)
    // `~'s equip ability costs {M} less/more to activate [if <cond>]` (`Ghostfire Blade`)
    {
        let ecp = ts.checkpoint();
        let matched_head = if ts.eat_phrase(&["equip", "abilities", "you", "activate"]) {
            if ts.eat_phrase(&["that", "target"]) {
                let _ = opt(ts, parse_subject_ref);
            }
            true
        } else if ts.eat_word("abilities") {
            let _ = ts.eat_phrase(&["your", "opponents", "activate"])
                || ts.eat_phrase(&["opponents", "activate"])
                || ts.eat_phrase(&["players", "activate"])
                || ts.eat_phrase(&["you", "activate"]);
            if ts.eat_phrase(&["that", "target"]) {
                let _ = opt(ts, parse_subject_ref);
            }
            true
        } else {
            ((ts.eat_phrase(&["activated", "abilities", "of"])
                || ts.eat_phrase(&["loyalty", "abilities", "of"]))
                && try_parse(ts, parse_object_filter).is_ok())
                || ts.eat_phrase(&["activated", "abilities"])
                || (ts.eat_tilde() && ts.eat_phrase(&["'s", "equip", "ability"]))
        };
        if matched_head
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
                let scale = opt(ts, parse_for_each_expr);
                let _ = ts.eat_phrase(&["unless", "they're", "mana", "abilities"])
                    || ts.eat_phrase(&["unless", "they", "are", "mana", "abilities"]);
                let extra_cond = if ts.eat_word("if") {
                    opt(ts, parse_condition)
                } else {
                    None
                };
                let final_cond = match (condition.clone(), extra_cond) {
                    (Some(prev), Some(c)) => Some(ConditionExpr::And(Box::new(prev), Box::new(c))),
                    (None, Some(c)) => Some(c),
                    (prev, None) => prev,
                };
                return Ok(StaticAbility::ActivationCostModifier {
                    delta,
                    less,
                    scale,
                    condition: final_cond,
                });
            }
        }
        ts.restore(ecp);
    }
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
                condition: None,
            });
        }
    }
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "the", "first", "spell", "each", "player", "casts", "during", "each", "of", "their",
            "turns", "may", "be", "cast", "without", "paying", "its", "mana", "cost",
        ]) {
            return Ok(StaticAbility::ZoneCastPermission {
                player: PlayerRef::EachPlayer,
                filter: None,
                subject: None,
                zone: None,
                mode: CastPermissionMode::FirstSpellFree,
                condition: None,
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
                if ts.eat_word("with") {
                    let _ = ts.eat_phrase(&["mana", "value"]);
                    let _ = opt(ts, parse_value_expr);
                    let _ = ts.eat_phrase(&["or", "less"]) || ts.eat_phrase(&["or", "greater"]);
                }
                if ts.eat_comma() && ts.eat_phrase(&["where", "x", "is"]) {
                    let _ = opt(ts, parse_value_expr);
                }
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
                    condition: None,
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
                if zone.is_some() && ts.eat_word("or") {
                    let _ = ts.eat_phrase(&["the", "top", "of"]);
                    let _ = opt(ts, parse_zone_ref);
                }
                let _ = ts.eat_phrase(&["this", "turn"]);
                let free = ts.eat_phrase(&["without", "paying", "its", "mana", "cost"])
                    || ts.eat_phrase(&["without", "paying", "their", "mana", "costs"]);
                let alt_cost = if !free && ts.eat_word("by") {
                    let costs = opt(ts, parse_cost_list);
                    let _ = ts
                        .eat_phrase(&["in", "addition", "to", "paying", "its", "other", "costs"])
                        || ts.eat_phrase(&["in", "addition", "to", "its", "other", "costs"])
                        || ts.eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"])
                        || ts.eat_phrase(&["rather", "than", "paying", "their", "mana", "costs"]);
                    costs
                } else {
                    None
                };
                let _ = ts.eat_phrase(&["rather", "than", "paying", "its", "mana", "cost"]);
                return Ok(StaticAbility::ZoneCastPermission {
                    player: PlayerRef::You,
                    filter: Some(filter),
                    subject: None,
                    zone,
                    mode: CastPermissionMode::OncePerTurn { free, alt_cost },
                    condition: None,
                });
            }
        }
        ts.restore(cp);
    }

    // 0aa. `You control enchanted creature/permanent`
    if ts.eat_phrase(&["you", "control", "enchanted"])
        && (ts.eat_word("creature")
            || ts.eat_word("permanent")
            || ts.eat_word("artifact")
            || ts.eat_word("land"))
    {
        return Ok(StaticAbility::ControlEnchanted {
            controller: PlayerRef::You,
        });
    }

    // 0ab. `The same is true for <flt> [and <flt>]` (`Encroaching Mycosynth`) / `The same is true for <kw_list>` (`Escaped Shapeshifter`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "same", "is", "true", "for"]) {
            if try_parse(ts, parse_object_filter).is_ok() {
                while ts.eat_word("and") || ts.eat_comma() {
                    let _ = ts.eat_word("and");
                    if try_parse(ts, parse_object_filter).is_err() {
                        break;
                    }
                }
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
                }
            }
            ts.restore(tcp);
            if ts.eat_phrase(&["the", "same", "is", "true", "for"])
                && try_parse(ts, crate::v2::filter::parse_keyword_list).is_ok()
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(StaticAbility::GlobalFlag(StaticFlagKind::AppliesInAllZones));
                }
            }
        }
        ts.restore(tcp);
    }
    if ts.eat_phrase(&["this", "effect", "doesn't", "remove"])
        && (ts.eat_tilde()
            || ts.eat_phrase(&[
                "auras",
                "and",
                "equipment",
                "you",
                "control",
                "that",
                "are",
                "already",
                "attached",
                "to",
                "it",
            ])
            || ts.eat_phrase(&["auras", "that", "are", "already", "attached", "to", "it"]))
    {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::RetainAttachment));
    }
    if ts.eat_phrase(&[
        "this", "effect", "reduces", "only", "the", "amount", "of", "colored", "mana", "you", "pay",
    ]) || ts.eat_phrase(&[
        "this", "effect", "can't", "reduce", "the", "mana", "in", "that", "cost", "to", "less",
        "than", "one", "mana",
    ]) || ts.eat_phrase(&[
        "this",
        "effect",
        "can't",
        "reduce",
        "the",
        "mana",
        "in",
        "that",
        "ability's",
        "activation",
        "cost",
        "to",
        "less",
        "than",
        "one",
        "mana",
    ]) || {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&[
            "this", "effect", "can't", "reduce", "the", "amount", "of", "mana",
        ]) && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"]))
            && ts.eat_phrase(&["costs", "by", "more", "than"])
            && ts.eat_mana_cost().is_some()
        {
            true
        } else {
            ts.restore(rcp);
            if ts.eat_phrase(&["this", "effect", "reduces", "only", "the", "amount", "of"])
                && ts
                    .peek_word()
                    .is_some_and(|w| crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some())
            {
                ts.next_word();
                if ts.eat_phrase(&["mana", "you", "pay"]) {
                    true
                } else {
                    ts.restore(rcp);
                    false
                }
            } else {
                ts.restore(rcp);
                false
            }
        }
    } {
        return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CostReductionRule));
    }
    // `As an additional cost to cast <flt>, you may pay <cost>.` (`Defiler of Vigor` cycle)
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["as", "an", "additional", "cost", "to", "cast"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_comma()
            && ts.eat_phrase(&["you", "may"])
            && let Ok(costs) = try_parse(ts, parse_cost_list)
        {
            return Ok(StaticAbility::AdditionalCost(costs));
        }
        ts.restore(acp);
    }
    // `Those spells cost {M} less/more to cast if you paid life this way.` (`Defiler of Vigor` cycle)
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["those", "spells", "cost"])
            && let Some(delta) = ts.eat_mana_cost()
            && (ts.eat_phrase(&["less", "to", "cast"]) || ts.eat_phrase(&["more", "to", "cast"]))
            && ts.eat_phrase(&["if", "you", "paid", "life", "this", "way"])
        {
            return Ok(StaticAbility::CostModifier {
                subject: ObjectFilter {
                    any_spell: true,
                    ..Default::default()
                },
                delta,
                less: true,
                scale: None,
                min_mana: None,
                condition: Some(ConditionExpr::EventCheck(
                    crate::v2::ir::EventCheckKind::PriorObjectEvent(
                        crate::v2::ir::PriorActionKind::LifePaidAsEntered,
                    ),
                )),
            });
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
            if ts.eat_phrase(&["gain", "life"]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantGainLife,
                    condition,
                });
            }
            if ts.eat_phrase(&["search", "libraries"]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantSearchLibraries,
                    condition,
                });
            }
            if ts.eat_phrase(&["draw", "more", "than", "one", "card", "each", "turn"]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantDrawMoreThanOne,
                    condition,
                });
            }
            if ts.eat_phrase(&["cast", "more", "than", "one", "spell", "each", "turn"]) {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantCastMoreThanOnePerTurn,
                    condition,
                });
            }
            if ts.eat_phrase(&["untap", "more", "than"])
                && let Some(n) = ts.next_number(false)
                && try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_phrase(&["during", "their", "untap", "steps"])
            {
                return Ok(StaticAbility::PlayerRestriction {
                    player,
                    restriction: PlayerRestrictionKind::CantUntapMoreThan(
                        u8::try_from(n).unwrap_or(1),
                    ),
                    condition,
                });
            }
            if ts.eat_phrase(&["cast", "spells"]) {
                if ts.eat_phrase(&["with", "the", "chosen", "name"]) {
                    let mut filter = ObjectFilter {
                        any_spell: true,
                        ..Default::default()
                    };
                    filter.predicates.push(FilterPredicate::OfChosenName);
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition,
                    });
                }
                if ts.eat_phrase(&["with", "the", "same", "name", "as"])
                    && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
                {
                    let mut filter = ObjectFilter {
                        any_spell: true,
                        ..Default::default()
                    };
                    filter.predicates.push(FilterPredicate::SharesPropertyWith {
                        property: crate::v2::ir::SharedProperty::Name,
                        reference: Box::new(ref_subj),
                        negated: false,
                    });
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition,
                    });
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
            // `<player> can't cast ~ if/unless <cond>` (`Rock Jockey`, `Rakdos, Lord of Riots`, `Proft, Sinister Mastermind`)
            {
                let sc_cp = ts.checkpoint();
                if ts.eat_word("cast") && (ts.eat_tilde() || ts.eat_phrase(&["this", "spell"])) {
                    let extra_cond = if ts.eat_word("if") {
                        opt(ts, parse_condition)
                    } else if ts.eat_word("unless") {
                        opt(ts, parse_condition).map(|c| ConditionExpr::Not(Box::new(c)))
                    } else {
                        None
                    };
                    if let Some(c) = extra_cond {
                        let merged = match condition {
                            Some(prev) => Some(ConditionExpr::And(Box::new(prev), Box::new(c))),
                            None => Some(c),
                        };
                        return Ok(StaticAbility::RuleRestriction {
                            subject: Some(SubjectRef::SelfObject),
                            rule: RuleRestrictionKind::CantBeCast,
                            condition: merged,
                        });
                    }
                }
                ts.restore(sc_cp);
            }
            // `<player> can't cast <flt> [from <zone> [or <zone2>]]` (`Narset Transcendent`, `Soulless Jailer`)
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
                    return Ok(StaticAbility::CantCastSpellsMatching {
                        player,
                        filter,
                        condition,
                    });
                }
                ts.restore(ccp);
            }
            // `<player> can't play lands or cast spells from your/their hand` (`Experimental Frenzy`)
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
            // `<player> can't win the game / lose the game [and <p2> can't ...]` (`Abyssal Persecutor`)
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

    // 0ac. `Prevent all [combat|noncombat] damage that would be dealt [to <subj>] [by <subj>]`
    if ts.eat_phrase(&["prevent", "all"]) {
        let combat_only = ts.eat_word("combat");
        let _ = ts.eat_word("noncombat");
        if ts.eat_word("damage") {
            let _ = ts.eat_phrase(&["that", "would", "be", "dealt"]);
            let mut source = None;
            let mut target = None;
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
        let scale = opt(ts, parse_for_each_expr);
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
        if ts.eat_tilde() {
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
            if ts.eat_phrase(&[
                "saddles", "mounts", "and", "crews", "vehicles", "as", "though", "its", "power",
                "were",
            ]) && let Some(n) = ts.next_number(false)
                && ts.eat_word("greater")
            {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::CrewPowerBonus(n)));
            }
            if ts.eat_phrase(&["enters", "prepared"]) {
                return Ok(StaticAbility::GlobalFlag(StaticFlagKind::EntersPrepared));
            }
            let _ = ts.eat_word("'s");
            if ts.eat_phrase(&["power", "and", "toughness", "are", "each", "equal", "to"]) {
                let val = parse_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: Some(val.clone()),
                    toughness: Some(val),
                });
            }
            if ts.eat_phrase(&["power", "is", "equal", "to"]) {
                let val = parse_value_expr(ts)?;
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
                        Some(parse_value_expr(ts)?)
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
                let val = parse_value_expr(ts)?;
                return Ok(StaticAbility::DynamicBasePT {
                    power: None,
                    toughness: Some(val),
                });
            }
        }
        ts.restore(cp);
    }

    // 0b. `Skip your <step>`
    if ts.eat_phrase(&["skip", "your"]) {
        let step = if ts.eat_phrase(&["draw", "step"]) {
            TurnStepAst::DrawStep
        } else if ts.eat_word("upkeep") {
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

    // 0b. Trigger-doubling static:
    //     `If <flt> entering [the battlefield] causes a triggered ability of <perm_flt> to trigger, that ability/it triggers an additional time`
    //     OR `If a triggered ability of <flt | ~> [or [another] <flt2>] triggers, that ability/it triggers an additional time`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["if", "a", "triggered", "ability", "of"]) {
            let source_filter_opt = if ts.eat_tilde() {
                Some(ObjectFilter::default())
            } else {
                try_parse(ts, parse_object_filter).ok()
            };
            if let Some(mut source_filter) = source_filter_opt {
                if ts.eat_word("or") {
                    let _ = ts.eat_word("another") || ts.eat_word("an") || ts.eat_word("a");
                    if let Ok(other) = try_parse(ts, parse_object_filter) {
                        source_filter = ObjectFilter {
                            predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![
                                source_filter,
                                other,
                            ])],
                            ..Default::default()
                        };
                    }
                }
                if ts.eat_word("triggers")
                    && ts.eat_comma()
                    && (ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
                        || ts.eat_phrase(&["it", "triggers", "an", "additional", "time"]))
                {
                    return Ok(StaticAbility::TriggerModifier {
                        kind: TriggerModifierKind::AdditionalOnAbility,
                        filter: source_filter,
                        permanent_filter: None,
                        extra_count: 1,
                    });
                }
            }
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "if",
            "a",
            "player",
            "drawing",
            "a",
            "card",
            "causes",
            "a",
            "triggered",
            "ability",
            "of",
        ]) && let Ok(permanent_filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["to", "trigger"])
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
        {
            return Ok(StaticAbility::TriggerModifier {
                kind: TriggerModifierKind::AdditionalTriggers,
                filter: ObjectFilter {
                    any_card: true,
                    ..Default::default()
                },
                permanent_filter: Some(permanent_filter),
                extra_count: 1,
            });
        }
        ts.restore(cp);
        if ts.eat_word("if")
            && let Ok(event_filter) = try_parse(ts, parse_object_filter)
            && (ts.eat_phrase(&[
                "entering",
                "the",
                "battlefield",
                "causes",
                "a",
                "triggered",
                "ability",
                "of",
            ]) || ts.eat_phrase(&["entering", "causes", "a", "triggered", "ability", "of"])
                || ts.eat_phrase(&["dying", "causes", "a", "triggered", "ability", "of"])
                || ts.eat_phrase(&["attacking", "causes", "a", "triggered", "ability", "of"])
                || ts.eat_phrase(&[
                    "dealing",
                    "combat",
                    "damage",
                    "to",
                    "a",
                    "player",
                    "causes",
                    "a",
                    "triggered",
                    "ability",
                    "of",
                ]))
            && let Ok(permanent_filter) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["to", "trigger"])
            && ts.eat_comma()
            && ts.eat_phrase(&["that", "ability", "triggers", "an", "additional", "time"])
        {
            return Ok(StaticAbility::TriggerModifier {
                kind: TriggerModifierKind::AdditionalTriggers,
                filter: event_filter,
                permanent_filter: Some(permanent_filter),
                extra_count: 1,
            });
        }
        ts.restore(cp);
    }

    // 0c. Player-headed static abilities (`player_rules.rs`)
    if let Ok(st) = try_parse(ts, |s| parse_player_static_ability(s, condition.clone())) {
        return Ok(st);
    }

    // 0d. `Reveal the first card you draw [on each of your turns | each turn]` (`Keranos, God of Storms`, `Primitive Etchings`, `Rowen`)
    if ts.eat_phrase(&["reveal", "the", "first", "card", "you", "draw"])
        && (ts.eat_phrase(&["on", "each", "of", "your", "turns"])
            || ts.eat_phrase(&["each", "turn"]))
    {
        return Ok(StaticAbility::VisibilityRule {
            player: PlayerRef::You,
            kind: VisibilityKind::PlayWithTopRevealed,
        });
    }
    // 0d. `Damage can't be prevented` / `The "legend rule" doesn't apply [to <flt>]` / `Activated abilities of sources with the chosen name can't be activated`
    if ts.eat_phrase(&["damage", "can't", "be", "prevented"]) {
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
    if ts.eat_phrase(&[
        "activated",
        "abilities",
        "of",
        "sources",
        "with",
        "the",
        "chosen",
        "name",
        "can't",
        "be",
        "activated",
    ]) {
        let _ = ts.eat_phrase(&["unless", "they're", "mana", "abilities"]);
        return Ok(StaticAbility::GlobalFlag(
            StaticFlagKind::NamedSourcesCantActivate,
        ));
    }
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["activated", "abilities", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && ts.eat_phrase(&["can't", "be", "activated"])
        {
            let _ = ts.eat_phrase(&["unless", "they're", "mana", "abilities"]);
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

    // 1z. `Spells and abilities can't be countered` (`Spider-Punk`)
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
            let mut subtypes = Vec::new();
            let mut saw_type = false;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if crate::vocab::lookup_color(&low).is_some() || low == "and" {
                    ts.next_word();
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
                if ts.eat_phrase(&[
                    "with",
                    "power",
                    "and",
                    "toughness",
                    "each",
                    "equal",
                    "to",
                    "its",
                    "mana",
                    "value",
                ]) {
                    let mv = ValueExpr::ObjectStat {
                        subject: Box::new(subj.clone()),
                        stat: crate::ast::StatKind::ManaValue,
                    };
                    parts.push(StaticAbility::DynamicBasePT {
                        power: Some(mv.clone()),
                        toughness: Some(mv),
                    });
                }
                let keep_existing = ts
                    .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                    || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"]);
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
                        && let Ok(rule) = try_parse(ts, subject_rules::parse_cant_rule)
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
                    if (ts.eat_phrase(&["and", "it", "has"]) || ts.eat_phrase(&["and", "has"]))
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
                return Ok(if parts.len() == 1 {
                    parts.remove(0)
                } else {
                    StaticAbility::Compound(parts)
                });
            }
        }
        ts.restore(itcp);
    }

    // 3. Subject-headed static abilities (`<subj> gets/get +X/+Y ...`, `<subj> can't ...`, etc.)
    subject_rules::parse_subject_static_ability(ts, condition)
}
