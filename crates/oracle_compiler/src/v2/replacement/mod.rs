//! Strictly-typed [`Replacement`] parser for `v2`.

mod if_would;
mod if_would_ext;

use crate::v2::clause::parse_effect_sentence;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{
    DamageReplacement, ObjectFilter, PlayerObjectTriggerKind, PlayerRef, PlayerTriggerKind,
    Replacement, ReplacementRuleKind, SubjectRef, TriggerEvent, ValueExpr,
};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::static_ability::cond_ext::{parse_static_condition, parse_static_value_expr};
use crate::v2::value::parse_value_expr;

pub(super) fn wrap_while_cond(
    while_cond: Option<crate::v2::ir::ConditionExpr>,
    inner: Replacement,
) -> Replacement {
    match while_cond {
        Some(condition) => Replacement::ConditionalEnters {
            condition,
            inner: Box::new(inner),
        },
        None => inner,
    }
}

pub(super) fn eat_period_eof(ts: &mut TokenStream<'_>) -> bool {
    let _ = ts.eat_period();
    ts.is_eof()
}

pub(super) fn parse_effect_sentences_commit(
    ts: &mut TokenStream<'_>,
) -> Option<Vec<crate::v2::ir::Effect>> {
    let cp = ts.checkpoint();
    let mut out = Vec::new();
    while !ts.is_eof() {
        let start = ts.pos;
        let mut end = start;
        while end < ts.tokens.len()
            && !matches!(ts.tokens[end].kind, crate::v2::lexer::TokenKind::Period)
        {
            end += 1;
        }
        if end == start {
            let _ = ts.eat_period();
            continue;
        }
        let mut sub = ts.sub_stream(&ts.tokens[start..end], crate::v2::ir::ParseStage::Effect);
        if let Ok(effs) = parse_effect_sentence(&mut sub)
            && sub.is_eof()
        {
            out.extend(effs);
            ts.commit_substream_eof(end, &sub);
            let _ = ts.eat_period();
        } else {
            ts.restore(cp);
            return None;
        }
    }
    if out.is_empty() {
        ts.restore(cp);
        return None;
    }
    Some(out)
}

fn grant_self_as_enters(compiled: crate::v2::ir::CompiledFaceV2) -> Replacement {
    Replacement::AsEnters(vec![crate::v2::ir::Effect::ApplyContinuous {
        effect: Box::new(crate::v2::ir::StaticAbility::GrantAbility {
            subject: SubjectRef::SelfObject,
            ability: Box::new(compiled),
            condition: None,
        }),
        duration: crate::v2::ir::Duration::Permanent,
    }])
}

fn wrap_optional(optional: bool, body: Vec<crate::v2::ir::Effect>) -> Vec<crate::v2::ir::Effect> {
    if optional {
        vec![crate::v2::ir::Effect::Optional {
            actor: PlayerRef::You,
            body,
        }]
    } else {
        body
    }
}

/// Parse a strictly-typed [`Replacement`] sentence (`As ~ enters, ...`,
/// `If <event> would ..., <instead> instead`,
/// `The next <flt> you cast this turn enters with ...`).
pub fn parse_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 0. `As [~ | it] enters [or is turned face up] / is turned face up / transforms into ~ / becomes attached to a creature, <body (one or more sentences)>`
    {
        let cp = ts.checkpoint();
        let matched_as = ts.eat_word("as")
            && (ts.eat_tilde() || ts.eat_word("it"))
            && (ts.eat_any_phrase(&[
                &["enters", "or", "is", "turned", "face", "up"],
                &["is", "turned", "face", "up"],
                &["becomes", "attached", "to", "a", "creature"],
            ]) || ts.eat_word("enters")
                || (ts.eat_phrase(&["transforms", "into"]) && ts.eat_tilde()))
            && ts.eat_comma();
        if matched_as {
            // `As ~ enters, you may have it become a copy of <flt> [, except ...]`
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["you", "may", "have", "it", "become", "a", "copy", "of"]) {
                    let _ = ts.eat_word("any");
                    if let Ok(filter) = try_parse(ts, parse_object_filter) {
                        let exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                        if eat_period_eof(ts) {
                            return Ok(Replacement::EnterAsCopy {
                                optional: true,
                                filter,
                                exceptions,
                            });
                        }
                    }
                }
                ts.restore(ccp);
            }
            // `As ~ enters [or is turned face up], it becomes your choice of <P/T ...>, ..., or <P/T ...>`
            {
                let bcp = ts.checkpoint();
                if ts.eat_phrase(&["it", "becomes", "your", "choice", "of"])
                    && let Some(branches) = parse_as_enters_pt_choices(ts)
                    && eat_period_eof(ts)
                {
                    return Ok(Replacement::AsEnters(vec![
                        crate::v2::ir::Effect::PromptChoice {
                            chooser: PlayerRef::You,
                            kind: crate::v2::ir::ChoiceKind::ChooseBranch(branches),
                        },
                    ]));
                }
                ts.restore(bcp);
            }
            // `As ~ enters, choose a basic land type / number / <Mode1> or <Mode2>`
            {
                let ch_cp = ts.checkpoint();
                if let Some(ch_eff) = parse_as_enters_characteristic_choice(ts)
                    && eat_period_eof(ts)
                {
                    return Ok(Replacement::AsEnters(vec![ch_eff]));
                }
                ts.restore(ch_cp);
            }
            let mut body = Vec::new();
            let mut s_idx = 0usize;
            while !ts.is_eof() {
                let start = ts.pos;
                let mut end = start;
                while end < ts.tokens.len()
                    && !matches!(ts.tokens[end].kind, crate::v2::lexer::TokenKind::Period)
                {
                    end += 1;
                }
                if end == start {
                    let _ = ts.eat_period();
                    continue;
                }
                let sl = &ts.tokens[start..end];
                let mut sub = ts.sub_stream(sl, crate::v2::ir::ParseStage::Effect);
                if let Ok(effs) = parse_effect_sentence(&mut sub)
                    && sub.is_eof()
                {
                    body.extend(effs);
                    ts.commit_substream_eof(end, &sub);
                    let _ = ts.eat_period();
                    s_idx += 1;
                    continue;
                }
                if s_idx > 0 {
                    let mut rts = ts.sub_stream(sl, crate::v2::ir::ParseStage::Replacement);
                    if let Ok(rep) = parse_replacement(&mut rts)
                        && rts.is_eof()
                    {
                        body.push(crate::v2::ir::Effect::ApplyReplacement {
                            replacement: Box::new(rep),
                            duration: crate::v2::ir::Duration::Permanent,
                        });
                        ts.commit_substream_eof(end, &rts);
                        let _ = ts.eat_period();
                        s_idx += 1;
                        continue;
                    }
                    let mut sts = ts.sub_stream(sl, crate::v2::ir::ParseStage::Static);
                    if let Ok(st) = crate::v2::static_ability::parse_static_ability(&mut sts)
                        && sts.is_eof()
                    {
                        body.push(crate::v2::ir::Effect::ApplyContinuous {
                            effect: Box::new(st),
                            duration: crate::v2::ir::Duration::Permanent,
                        });
                        ts.commit_substream_eof(end, &sts);
                        let _ = ts.eat_period();
                        s_idx += 1;
                        continue;
                    }
                }
                let mut err_sub = ts.sub_stream(sl, crate::v2::ir::ParseStage::Effect);
                let _ = parse_effect_sentence(&mut err_sub)?;
                err_sub.expect_eof()?;
            }
            if body.is_empty() {
                ts.restore(cp);
                return Err(ts.err("empty `As ~ enters` body"));
            }
            return Ok(Replacement::AsEnters(body));
        }
        ts.restore(cp);
    }

    // 0a. `Double all damage <subj> would deal.`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["double", "all", "damage"]) {
            let _ = ts.eat_word("that");
            if let Ok(src_flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "deal"])
                && eat_period_eof(ts)
            {
                return Ok(Replacement::ModifyDamage {
                    source: Some(src_flt),
                    target: None,
                    add: 0,
                    factor: 2,
                });
            }
            ts.restore(dcp);
            if ts.eat_phrase(&["double", "all", "damage"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["would", "deal"])
                && eat_period_eof(ts)
            {
                return Ok(Replacement::ModifyDamage {
                    source: None,
                    target: None,
                    add: 0,
                    factor: 2,
                });
            }
        }
        ts.restore(dcp);
    }

    // 0b. `You may have ~ enter [the battlefield] as a copy of <flt> [, except ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "have"]) && ts.eat_tilde() && ts.eat_word("enter") {
            let _ = ts.eat_phrase(&["the", "battlefield"]);
            if ts.eat_phrase(&["as", "a", "copy", "of"]) {
                let _ = ts.eat_word("any");
                if let Ok(filter) = try_parse(ts, parse_object_filter) {
                    let exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                    if eat_period_eof(ts) {
                        return Ok(Replacement::EnterAsCopy {
                            optional: true,
                            filter,
                            exceptions,
                        });
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 0c. `[All] damage that would ...`
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["damage", "that", "would", "reduce"])
            && let Ok(crate::v2::ir::ValueExpr::PlayerStat {
                player: PlayerRef::You,
                stat: crate::ast::PlayerStatKind::LifeTotal,
            }) = try_parse(ts, parse_value_expr)
            && ts.eat_phrase(&["to", "less", "than"])
            && let Some(floor) = ts.next_number(false)
            && ts.eat_phrase(&["reduces", "it", "to"])
            && ts.next_number(false) == Some(floor)
            && ts.eat_word("instead")
            && eat_period_eof(ts)
        {
            return Ok(Replacement::Rule(ReplacementRuleKind::LifeFloor(floor)));
        }
        ts.restore(cp);
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "by"])
            && let Ok(src) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["can't", "be", "prevented"])
            && eat_period_eof(ts)
        {
            return Ok(Replacement::DamageRule(
                DamageReplacement::UnpreventableBySource(src),
            ));
        }
        ts.restore(cp);
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "to"])
            && let Ok(from) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["is", "dealt", "to"])
            && let Ok(to) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("instead")
            && eat_period_eof(ts)
        {
            return Ok(Replacement::DamageRule(DamageReplacement::RedirectAll {
                from,
                to,
            }));
        }
        ts.restore(cp);
    }

    // 1. `If ... would ...`
    {
        let if_cp = ts.checkpoint();
        if ts.eat_word("if")
            && let Ok(rep) = try_parse(ts, if_would::parse_if_would_replacement)
        {
            return Ok(rep);
        }
        ts.restore(if_cp);
    }

    // 2a0. `The first time <player> would draw a card / create one or more tokens each turn, [you may] <instead>`
    {
        let cp = ts.checkpoint();
        let first_time_cond =
            || crate::v2::ir::ConditionExpr::Flag(crate::v2::ir::ConditionFlag::FirstTimeEachTurn);
        if ts.eat_phrase(&["the", "first", "time"])
            && let Ok(PlayerRef::You) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && ts.eat_word("would")
        {
            let ev = if ts.eat_phrase(&["draw", "a", "card"]) {
                Some(TriggerEvent::PlayerAction {
                    player: PlayerRef::You,
                    action: PlayerTriggerKind::CardDrawn,
                })
            } else if ts.eat_phrase(&["create", "one", "or", "more", "tokens"]) {
                Some(TriggerEvent::PlayerObjectEvent {
                    player: PlayerRef::You,
                    kind: PlayerObjectTriggerKind::CreatesTokens,
                    filter: Some(ObjectFilter {
                        token: Some(true),
                        ..Default::default()
                    }),
                })
            } else {
                None
            };
            if let Some(event) = ev
                && ts.eat_phrase(&["each", "turn"])
                && ts.eat_comma()
            {
                let optional = ts.eat_phrase(&["you", "may"]);
                if let Ok(instead) = parse_instead_clause(ts)
                    && eat_period_eof(ts)
                {
                    return Ok(Replacement::ConditionalEnters {
                        condition: first_time_cond(),
                        inner: Box::new(Replacement::EventInstead {
                            event,
                            instead: wrap_optional(optional, instead),
                        }),
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // 2a1. `The next <val> damage that would be dealt to <from> this turn is dealt to <to> instead.`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next"])
            && let Ok(amount) = try_parse(ts, parse_value_expr)
            && ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "to"])
            && let Ok(from) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_phrase(&["this", "turn"]);
            if ts.eat_phrase(&["is", "dealt", "to"])
                && let Ok(to) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("instead")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::DamageRule(DamageReplacement::RedirectNext {
                        amount,
                        from,
                        to,
                    }));
                }
            }
        }
        ts.restore(cp);
    }

    // 2a. `The next time ... [this turn], <instead>` (damage redirect/prevent/replace, draw replacement, lose-game replacement)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next", "time"]) {
            // 2a-draw: `The next time you would draw a card this turn, <instead>`
            {
                let dcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "would", "draw", "a", "card"]) {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                        && eat_period_eof(ts)
                    {
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player: PlayerRef::You,
                                action: PlayerTriggerKind::CardDrawn,
                            },
                            instead,
                        });
                    }
                }
                ts.restore(dcp);
            }
            // 2a-lose: `The next time you would lose the game this turn, <instead>`
            {
                let lcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "would", "lose", "the", "game"]) {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                        && eat_period_eof(ts)
                    {
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::PlayerAction {
                                player: PlayerRef::You,
                                action: PlayerTriggerKind::LosesGame,
                            },
                            instead,
                        });
                    }
                }
                ts.restore(lcp);
            }
            // 2a-dmg-to: `The next time [combat] damage would be dealt to <tgt> this turn, <instead>`
            {
                let d2cp = ts.checkpoint();
                let combat_only = ts.eat_word("combat");
                if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
                    && let Ok(tgt) = try_parse(ts, parse_subject_ref)
                {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                        && eat_period_eof(ts)
                    {
                        return Ok(Replacement::EventInstead {
                            event: TriggerEvent::DealtDamage {
                                subject: tgt,
                                combat_only,
                            },
                            instead,
                        });
                    }
                }
                ts.restore(d2cp);
            }
            let mut src_flt = ObjectFilter::default();
            let mut parsed_source = false;
            {
                let scp = ts.checkpoint();
                if ts.eat_word("a") || ts.eat_word("an") {
                    if let Some(w) = ts.peek_word()
                        && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
                    {
                        ts.next_word();
                        src_flt.any_colors = src_flt.any_colors.union(c);
                    } else if let Some(w) = ts.peek_word()
                        && let Some(ct) = crate::vocab::lookup_card_type(&w.to_ascii_lowercase())
                    {
                        ts.next_word();
                        src_flt.any_types = src_flt.any_types.union(ct);
                    }
                    if ts.eat_word("source") {
                        parsed_source = true;
                        let _ = ts.eat_phrase(&["of", "your", "choice"]);
                        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
                            src_flt
                                .predicates
                                .push(crate::v2::ir::FilterPredicate::OfChosenColor);
                        } else if ts.eat_phrase(&["you", "control"]) {
                            src_flt.controller = Some(crate::ast::ControllerRelation::You);
                        }
                    }
                }
                if !parsed_source {
                    ts.restore(scp);
                }
            }
            if !parsed_source && let Ok(f) = try_parse(ts, parse_object_filter) {
                src_flt = f;
                parsed_source = true;
            }
            if parsed_source && ts.eat_phrase(&["would", "deal", "damage"]) {
                let target = if ts.eat_word("to") {
                    opt(ts, parse_subject_ref)
                } else {
                    None
                };
                let _ = ts.eat_phrase(&["this", "turn"]);
                if ts.eat_comma() {
                    // `prevent half that damage, rounded down/up`
                    {
                        let hcp = ts.checkpoint();
                        if ts.eat_phrase(&["prevent", "half", "that", "damage"]) {
                            let rounding = if ts.eat_rounded_modifier() == Some(true) {
                                crate::ast::Rounding::Up
                            } else {
                                crate::ast::Rounding::Down
                            };
                            if eat_period_eof(ts) {
                                return Ok(Replacement::DamageRule(
                                    DamageReplacement::PreventFixed {
                                        source: Some(src_flt),
                                        target,
                                        amount: ValueExpr::Div {
                                            expr: Box::new(ValueExpr::PriorActionCount(
                                                crate::v2::ir::PriorActionKind::ThatMany,
                                            )),
                                            denom: 2,
                                            rounding,
                                        },
                                    },
                                ));
                            }
                        }
                        ts.restore(hcp);
                    }
                    // `that damage is dealt to <to> instead` (only when unfiltered source & explicit target)
                    if src_flt == ObjectFilter::default()
                        && let Some(from) = target.clone()
                    {
                        let rcp = ts.checkpoint();
                        if ts.eat_any_phrase(&[
                            &["that", "damage", "is", "dealt", "to"],
                            &["that", "source", "deals", "that", "damage", "to"],
                            &["that", "creature", "deals", "that", "damage", "to"],
                            &["it", "deals", "that", "damage", "to"],
                        ]) && let Ok(to) = try_parse(ts, parse_subject_ref)
                            && ts.eat_word("instead")
                            && eat_period_eof(ts)
                        {
                            return Ok(Replacement::DamageRule(DamageReplacement::RedirectAll {
                                from,
                                to,
                            }));
                        }
                        ts.restore(rcp);
                    }
                    {
                        let pcp = ts.checkpoint();
                        if ts.eat_word("prevent")
                            && let Ok(amount) = try_parse(ts, parse_value_expr)
                            && ts.eat_phrase(&["of", "that", "damage"])
                            && eat_period_eof(ts)
                        {
                            return Ok(Replacement::DamageRule(DamageReplacement::PreventFixed {
                                source: Some(src_flt),
                                target,
                                amount,
                            }));
                        }
                        ts.restore(pcp);
                    }
                    if let Ok(instead) = parse_instead_clause(ts) {
                        let _ = ts.eat_period();
                        ts.expect_eof()?;
                        return Ok(Replacement::DamageRule(DamageReplacement::ReplaceNext {
                            source: Some(src_flt),
                            target,
                            instead,
                        }));
                    }
                }
            }
        }
        ts.restore(cp);
    }

    // 2. `The next <flt> you cast this turn enters with [a number of] [additional] <kind> counters on it equal to <val>`
    if ts.eat_phrase(&["the", "next"]) {
        let flt = parse_object_filter(ts)?;
        if ts.eat_phrase(&["you", "cast", "this", "turn", "enters", "with"])
            || ts.eat_phrase(&["this", "turn", "enters", "with"])
        {
            let _ = ts.eat_phrase(&["a", "number", "of"]);
            let additional = ts.eat_word("additional");
            let kind = parse_counter_kind(ts)?;
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["on", "it", "equal", "to"])
            {
                let count = parse_value_expr(ts)?;
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::EntersWithCounters {
                    subject: Some(SubjectRef::Group(Box::new(flt))),
                    kind,
                    count,
                    additional,
                });
            }
        }
        return Err(ts.err("unsupported `The next ... enters with ...` replacement"));
    }

    // 3. `~ escapes with ...` / `~ enters tapped [unless <cond>]` / `~ enters with ...`
    if ts.eat_tilde() || ts.eat_word("it") {
        if ts.eat_phrase(&["escapes", "with"]) {
            if let Some(quoted) = ts.next_quoted()
                && let Ok(compiled) = ts.compile_quoted_permanent_ability(quoted)
            {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(grant_self_as_enters(compiled));
            }
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["your", "choice", "of"])
                    && ts.eat_any_word(&["a", "an"])
                    && let Ok(kind1) = try_parse(ts, parse_counter_kind)
                    && ts.eat_any_word(&["counter", "counters"])
                    && ts.eat_word("or")
                    && ts.eat_any_word(&["a", "an"])
                    && let Ok(kind2) = try_parse(ts, parse_counter_kind)
                    && ts.eat_any_word(&["counter", "counters"])
                    && ts.eat_phrase(&["on", "it"])
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::Rule(
                        ReplacementRuleKind::EntersWithChoiceCounter { kind1, kind2 },
                    ));
                }
                ts.restore(ccp);
            }
            let count = if ts.eat_any_word(&["a", "an"]) {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let kind = parse_counter_kind(ts)?;
            if ts.eat_any_word(&["counters", "counter"]) && ts.eat_phrase(&["on", "it"]) {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::Rule(
                    ReplacementRuleKind::EscapesWithCounters { kind, count },
                ));
            }
        }
        if ts.eat_phrase(&["enters", "tapped"]) {
            // Support `~ enters tapped [and] with N <kind> counters on it`
            let wcp = ts.checkpoint();
            if ts.eat_phrase(&["and", "with"]) || ts.eat_word("with") {
                let count_opt = if ts.eat_word("a") || ts.eat_word("an") {
                    Some(ValueExpr::Const(1))
                } else {
                    try_parse(ts, parse_value_expr).ok()
                };
                if let Some(count) = count_opt
                    && let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["on", "it"])
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::Compound(vec![
                        Replacement::EntersTapped { unless: None },
                        Replacement::EntersWithCounters {
                            subject: None,
                            kind,
                            count,
                            additional: false,
                        },
                    ]));
                }
                ts.restore(wcp);
            }
            let unless = if ts.eat_word("unless") {
                Some(parse_static_condition(ts)?)
            } else {
                None
            };
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EntersTapped { unless });
        }
        if ts.eat_phrase(&["enters", "with"]) {
            // `~ enters with your choice of a/an <k1> counter or a/an <k2> counter on it`
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["your", "choice", "of"])
                    && (ts.eat_word("a") || ts.eat_word("an"))
                    && let Ok(kind1) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && ts.eat_word("or")
                    && (ts.eat_word("a") || ts.eat_word("an"))
                    && let Ok(kind2) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                    && ts.eat_phrase(&["on", "it"])
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::Rule(
                        ReplacementRuleKind::EntersWithChoiceCounter { kind1, kind2 },
                    ));
                }
                ts.restore(ccp);
            }
            // `~ enters with <kws> and with <count> <kind> counters on it [, where X is ...]`
            let mut leading_kws: Option<Vec<crate::v2::ir::KeywordSpec>> = None;
            {
                let lkw_cp = ts.checkpoint();
                if let Ok((kws, _)) = try_parse(ts, crate::v2::keywords::parse_keyword_list)
                    && ts.eat_phrase(&["and", "with"])
                {
                    leading_kws = Some(kws);
                } else {
                    ts.restore(lkw_cp);
                }
            }
            if let Ok(rep) = parse_enters_with_counters_body(ts, leading_kws) {
                return Ok(rep);
            }
        }
    }

    Err(ts.err("expected replacement effect"))
}

pub(super) fn parse_enters_with_counters_body(
    ts: &mut TokenStream<'_>,
    leading_kws: Option<Vec<crate::v2::ir::KeywordSpec>>,
) -> PResult<Replacement> {
    let mut counter_entries: Vec<Replacement> = Vec::new();
    let mut deferred_equal_to = false;
    loop {
        let mut additional = false;
        let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
            deferred_equal_to = true;
            None
        } else if ts.eat_phrase(&["an", "additional"]) {
            additional = true;
            Some(ValueExpr::Const(1))
        } else if ts.eat_word("a") || ts.eat_word("an") {
            Some(ValueExpr::Const(1))
        } else {
            Some(parse_value_expr(ts)?)
        };
        if ts.eat_word("additional") {
            additional = true;
        }
        let kind = parse_counter_kind(ts)?;
        if !(ts.eat_word("counters") || ts.eat_word("counter")) {
            return Err(ts.err("expected `counter` or `counters`"));
        }
        let had_on_it = ts.eat_phrase(&["on", "it"]);
        let mut count = if let Some(c) = count_opt {
            if let Some(scale) = parse_enters_scale_expr(ts) {
                if c == ValueExpr::Const(1) {
                    scale
                } else {
                    ValueExpr::Mul(Box::new(c), Box::new(scale))
                }
            } else {
                c
            }
        } else if deferred_equal_to && ts.eat_phrase(&["equal", "to"]) {
            parse_static_value_expr(ts)?
        } else {
            return Err(
                ts.err("expected `equal to <val>` after `a number of <kind> counters on it`")
            );
        };
        {
            let wcp = ts.checkpoint();
            if ts.eat_comma() && ts.eat_phrase(&["where", "x", "is"]) {
                count = parse_static_value_expr(ts)?;
            } else {
                ts.restore(wcp);
            }
        }
        counter_entries.push(Replacement::EntersWithCounters {
            subject: None,
            kind,
            count,
            additional,
        });
        // Multi-counter list: `, a <kind> counter`, `and a <kind> counter`, `plus an additional <kind> counter on it`
        let next_cp = ts.checkpoint();
        if !had_on_it && (ts.eat_comma() || ts.eat_word("and")) {
            let _ = ts.eat_word("and");
            continue;
        }
        if had_on_it
            && (ts.eat_phrase(&["plus", "an", "additional"]) || ts.eat_word("and"))
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let probe_cp = ts.checkpoint();
            if ts.peek_word().is_some_and(|w| {
                let l = w.to_ascii_lowercase();
                l == "a"
                    || l == "an"
                    || l == "+1/+1"
                    || crate::vocab::parse_number_word(&l).is_some()
            }) || matches!(
                ts.peek().map(|t| &t.kind),
                Some(
                    crate::v2::lexer::TokenKind::Number(_)
                        | crate::v2::lexer::TokenKind::PtMod(_, _)
                )
            ) {
                continue;
            }
            ts.restore(probe_cp);
        }
        ts.restore(next_cp);
        if !had_on_it {
            return Err(ts.err("expected `on it` after counters"));
        }
        break;
    }
    if let Some(kws) = leading_kws {
        counter_entries.push(Replacement::Rule(ReplacementRuleKind::EntersWithKeywords(
            kws,
        )));
    }
    let kwcp = ts.checkpoint();
    if ts.eat_phrase(&["and", "with"]) {
        if let Some(quoted) = ts.next_quoted() {
            if let Ok(compiled) = ts.compile_quoted_permanent_ability(quoted) {
                counter_entries.push(grant_self_as_enters(compiled));
            } else {
                ts.restore(kwcp);
            }
        } else if let Ok((kws, _)) = try_parse(ts, crate::v2::keywords::parse_keyword_list) {
            counter_entries.push(Replacement::Rule(ReplacementRuleKind::EntersWithKeywords(
                kws,
            )));
        } else {
            ts.restore(kwcp);
        }
    }
    let cond = if ts.eat_word("if") {
        Some(parse_static_condition(ts)?)
    } else if ts.eat_word("unless") {
        Some(crate::v2::ir::ConditionExpr::Not(Box::new(
            parse_static_condition(ts)?,
        )))
    } else {
        None
    };
    let _ = ts.eat_period();
    ts.expect_eof()?;
    let base = if counter_entries.len() == 1 {
        counter_entries.remove(0)
    } else {
        Replacement::Compound(counter_entries)
    };
    Ok(wrap_while_cond(cond, base))
}

fn parse_enters_scale_expr(ts: &mut TokenStream<'_>) -> Option<ValueExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["for", "each", "counter", "removed", "this", "way"]) {
        return Some(ValueExpr::PriorActionCount(
            crate::v2::ir::PriorActionKind::CountersRemovedThisWay,
        ));
    }
    if ts.eat_phrase(&["for", "each", "other", "spell", "cast", "this", "turn"]) {
        return Some(ValueExpr::EventCountThisTurn {
            event: crate::v2::ir::TurnEventKind::SpellCast,
            player: None,
            exclude_first: true,
        });
    }
    if ts.eat_phrase(&["for", "each", "mana", "spent", "to", "cast", "it"]) {
        return Some(ValueExpr::ManaPaidThisWay);
    }
    if ts.eat_phrase(&["for", "each", "card", "exiled", "this", "way"]) {
        return Some(ValueExpr::PriorActionCount(
            crate::v2::ir::PriorActionKind::ExiledThisWay,
        ));
    }
    ts.restore(cp);
    opt(ts, crate::v2::value::parse_for_each_expr)
}

fn parse_as_enters_pt_choices(ts: &mut TokenStream<'_>) -> Option<Vec<Vec<crate::v2::ir::Effect>>> {
    let mut branches: Vec<Vec<crate::v2::ir::Effect>> = Vec::new();
    loop {
        let _ = ts.eat_word("a") || ts.eat_word("an");
        let (p, t) = ts.next_base_pt()?;
        let mut subtypes = Vec::new();
        let mut add_types = crate::ast::CardTypeMask::EMPTY;
        while let Some(w) = ts.peek_word() {
            let low = w.to_ascii_lowercase();
            if let Some(ct) = crate::vocab::lookup_card_type(&low) {
                ts.next_word();
                add_types = add_types.union(ct);
            } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                ts.next_word();
                subtypes.push(sub.to_string());
            } else {
                break;
            }
        }
        let keywords = if ts.eat_word("with") {
            try_parse(ts, crate::v2::filter::parse_keyword_list)
                .map(|(k, _)| k)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let keep_existing_types = ts.eat_in_addition_to_other_types();
        branches.push(vec![crate::v2::ir::Effect::MutateObject {
            subject: SubjectRef::SelfObject,
            mutation: crate::v2::ir::ObjectMutation::BecomeCreatureTypes {
                subtypes,
                add_types,
                keep_existing_types,
                base_pt: Some((p, t)),
                keywords,
            },
            duration: crate::v2::ir::Duration::Permanent,
        }]);
        let had_comma = ts.eat_comma();
        let had_or = ts.eat_word("or");
        if had_comma || had_or {
            continue;
        }
        break;
    }
    if branches.len() >= 2 {
        Some(branches)
    } else {
        None
    }
}

fn parse_as_enters_characteristic_choice(
    ts: &mut TokenStream<'_>,
) -> Option<crate::v2::ir::Effect> {
    use crate::v2::ir::{CharacteristicKind, ChoiceKind, Effect};
    if !ts.eat_word("choose") {
        return None;
    }
    let kind = if ts.eat_phrase(&["a", "basic", "land", "type"]) {
        CharacteristicKind::BasicLandType
    } else if ts.eat_phrase(&["a", "number"]) {
        CharacteristicKind::Number
    } else if let Some(w1) = ts.peek_word()
        && w1.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    {
        let cp = ts.checkpoint();
        ts.next_word();
        if ts.eat_word("or")
            && let Some(w2) = ts.peek_word()
            && w2.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        {
            ts.next_word();
            CharacteristicKind::BinaryMode
        } else {
            ts.restore(cp);
            return None;
        }
    } else {
        return None;
    };
    Some(Effect::PromptChoice {
        chooser: PlayerRef::You,
        kind: ChoiceKind::ChooseCharacteristic(kind),
    })
}

pub(super) fn lookup_predefined_token(low: &str) -> Option<crate::v2::ir::PredefinedToken> {
    use crate::v2::ir::PredefinedToken;
    match low {
        "treasure" => Some(PredefinedToken::Treasure),
        "clue" => Some(PredefinedToken::Clue),
        "food" => Some(PredefinedToken::Food),
        "blood" => Some(PredefinedToken::Blood),
        "map" => Some(PredefinedToken::Map),
        "powerstone" => Some(PredefinedToken::Powerstone),
        "shard" => Some(PredefinedToken::Shard),
        "lander" => Some(PredefinedToken::Lander),
        "junk" => Some(PredefinedToken::Junk),
        "gold" => Some(PredefinedToken::Gold),
        "incubator" => Some(PredefinedToken::Incubator),
        "mutagen" => Some(PredefinedToken::Mutagen),
        _ => None,
    }
}

pub(super) fn parse_instead_clause(
    ts: &mut TokenStream<'_>,
) -> PResult<Vec<crate::v2::ir::Effect>> {
    // Support both leading `instead <effect>` and trailing `<effect> instead`
    if ts.eat_word("instead") {
        let start = ts.pos;
        let mut end = ts.tokens.len();
        if end > start && matches!(ts.tokens[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
            end -= 1;
        }
        let body_slice = &ts.tokens[start..end];
        let mut sub = ts.sub_stream(body_slice, ts.stage);
        let effects = parse_effect_sentence(&mut sub)?;
        sub.expect_eof()?;
        if effects.is_empty() {
            return Err(ts.err("empty replacement `instead` clause"));
        }
        ts.commit_substream_eof(end, &sub);
        return Ok(effects);
    }
    let start = ts.pos;
    let mut end = ts.tokens.len();
    if end > start && matches!(ts.tokens[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
        end -= 1;
    }
    let remaining = &ts.tokens[start..end];
    // Support trailing `instead of putting it/them anywhere else` / `instead of putting it/them into <zone> [as it resolves]`
    let mut instead_idx: Option<usize> = None;
    for i in (0..remaining.len()).rev() {
        if matches!(
            remaining[i].kind,
            crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        ) {
            if i + 1 == remaining.len() {
                instead_idx = Some(i);
                break;
            }
            let tail = &remaining[i + 1..];
            let mut tail_ts = ts.sub_stream(tail, ts.stage);
            let ok_tail = (tail_ts.eat_phrase(&["of", "putting", "it", "anywhere", "else"])
                || tail_ts.eat_phrase(&["of", "putting", "them", "anywhere", "else"])
                || ((tail_ts.eat_phrase(&["of", "putting", "it", "into"])
                    || tail_ts.eat_phrase(&["of", "putting", "them", "into"]))
                    && try_parse(&mut tail_ts, crate::v2::filter::parse_zone_ref).is_ok()
                    && {
                        let _ = tail_ts.eat_phrase(&["as", "it", "resolves"]);
                        true
                    }))
                && tail_ts.is_eof();
            if ok_tail {
                instead_idx = Some(i);
                break;
            }
        }
    }
    let Some(idx) = instead_idx else {
        return Err(ts.err("expected trailing `instead` on replacement clause"));
    };
    let body_slice = &remaining[..idx];
    let mut sub = ts.sub_stream(body_slice, ts.stage);
    let effects = parse_effect_sentence(&mut sub)?;
    sub.expect_eof()?;
    if effects.is_empty() {
        return Err(ts.err("empty replacement `instead` clause"));
    }
    ts.commit_substream_eof(start + idx, &sub);
    if !ts.eat_word("instead") {
        return Err(ts.err("expected `instead`"));
    }
    if start + idx + 1 < end {
        let tail_start = ts.pos;
        let mut tail_ts = ts.sub_stream(&ts.tokens[tail_start..end], ts.stage);
        let ok_tail = (tail_ts.eat_phrase(&["of", "putting", "it", "anywhere", "else"])
            || tail_ts.eat_phrase(&["of", "putting", "them", "anywhere", "else"])
            || ((tail_ts.eat_phrase(&["of", "putting", "it", "into"])
                || tail_ts.eat_phrase(&["of", "putting", "them", "into"]))
                && try_parse(&mut tail_ts, crate::v2::filter::parse_zone_ref).is_ok()
                && {
                    let _ = tail_ts.eat_phrase(&["as", "it", "resolves"]);
                    true
                }))
            && tail_ts.is_eof();
        if !ok_tail {
            return Err(ts.err("unexpected trailing tokens after `instead`"));
        }
        ts.commit_substream_eof(end, &tail_ts);
    }
    Ok(effects)
}
