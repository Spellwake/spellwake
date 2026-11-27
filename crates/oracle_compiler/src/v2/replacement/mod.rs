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

/// Parse a strictly-typed [`Replacement`] sentence (`As ~ enters, ...`,
/// `If <event> would ..., <instead> instead`,
/// `The next <flt> you cast this turn enters with ...`).
pub fn parse_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 0. `As [~ | it | <flt>] enters [or is turned face up] / is turned face up / transforms into ~ / becomes attached to a creature, <body (one or more sentences)>`
    {
        let cp = ts.checkpoint();
        let matched_as = if ts.eat_word("as") {
            if (ts.eat_tilde() || ts.eat_word("it"))
                && (ts.eat_phrase(&["enters", "or", "is", "turned", "face", "up"])
                    || ts.eat_word("enters")
                    || ts.eat_phrase(&["is", "turned", "face", "up"])
                    || ts.eat_phrase(&["becomes", "attached", "to", "a", "creature"])
                    || (ts.eat_phrase(&["transforms", "into"]) && ts.eat_tilde()))
                && ts.eat_comma()
            {
                true
            } else {
                ts.restore(cp);
                ts.eat_word("as")
                    && try_parse(ts, parse_object_filter).is_ok()
                    && ts.eat_word("enters")
                    && ts.eat_comma()
            }
        } else {
            false
        };
        if matched_as {
            // `As ~ enters, you may have it become a copy of <flt> [until end of turn] [, except ...]`
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["you", "may", "have", "it", "become", "a", "copy", "of"]) {
                    let _ = ts.eat_word("any");
                    if let Ok(filter) = try_parse(ts, parse_object_filter) {
                        let _ = ts.eat_phrase(&["until", "end", "of", "turn"]);
                        let mut exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                        parse_extra_copy_exceptions(ts, &mut exceptions);
                        let _ = ts.eat_period();
                        if ts.is_eof() {
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
                {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::AsEnters(vec![
                            crate::v2::ir::Effect::PromptChoice {
                                chooser: PlayerRef::You,
                                kind: crate::v2::ir::ChoiceKind::ChooseBranch(branches),
                            },
                        ]));
                    }
                }
                ts.restore(bcp);
            }
            // `As ~ enters, choose a [land type | number | ...] / choose <Mode1> or <Mode2> / special multi-sentence As-enters bodies`
            {
                let ch_cp = ts.checkpoint();
                if let Some(ch_eff) = parse_as_enters_characteristic_choice(ts) {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::AsEnters(vec![ch_eff]));
                    }
                }
                ts.restore(ch_cp);
                if let Some(rep) = parse_as_enters_special_body(ts) {
                    return Ok(rep);
                }
                ts.restore(ch_cp);
            }
            let mut body = Vec::new();
            let mut cur_start = ts.pos;
            let mut sentences: Vec<(usize, usize)> = Vec::new();
            for idx in ts.pos..ts.tokens.len() {
                if matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Period) {
                    if idx > cur_start {
                        sentences.push((cur_start, idx));
                    }
                    cur_start = idx + 1;
                }
            }
            if cur_start < ts.tokens.len() {
                sentences.push((cur_start, ts.tokens.len()));
            }
            for (s_idx, (a, b)) in sentences.into_iter().enumerate() {
                let sl = &ts.tokens[a..b];
                let mut sub = ts.sub_stream(sl, crate::v2::ir::ParseStage::Effect);
                if let Ok(effs) = parse_effect_sentence(&mut sub)
                    && sub.is_eof()
                {
                    body.extend(effs);
                    continue;
                }
                if s_idx > 0 {
                    let mut rts = ts.sub_stream(sl, crate::v2::ir::ParseStage::Replacement);
                    if let Ok(rep) = parse_replacement(&mut rts) {
                        body.push(crate::v2::ir::Effect::ApplyReplacement {
                            replacement: Box::new(rep),
                            duration: crate::v2::ir::Duration::Permanent,
                        });
                        continue;
                    }
                    let mut sts = ts.sub_stream(sl, crate::v2::ir::ParseStage::Static);
                    if let Ok(st) = crate::v2::static_ability::parse_static_ability(&mut sts) {
                        body.push(crate::v2::ir::Effect::ApplyContinuous {
                            effect: Box::new(st),
                            duration: crate::v2::ir::Duration::Permanent,
                        });
                        continue;
                    }
                }
                let mut err_sub = ts.sub_stream(sl, crate::v2::ir::ParseStage::Effect);
                let _ = parse_effect_sentence(&mut err_sub)?;
                err_sub.expect_eof()?;
            }
            ts.pos = ts.tokens.len();
            return Ok(Replacement::AsEnters(body));
        }
        ts.restore(cp);
    }

    // 0a. `Double all damage <subj> would deal.` (`Wolverine, Best There Is`, `Collective Inferno`)
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["double", "all", "damage"]) {
            let _ = ts.eat_word("that");
            if let Ok(src_flt) = try_parse(ts, parse_object_filter)
                && ts.eat_phrase(&["would", "deal"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ModifyDamage {
                        source: Some(src_flt),
                        target: None,
                        add: 0,
                        factor: 2,
                    });
                }
            }
            ts.restore(dcp);
            if ts.eat_phrase(&["double", "all", "damage"])
                && ts.eat_tilde()
                && ts.eat_phrase(&["would", "deal"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::ModifyDamage {
                        source: None,
                        target: None,
                        add: 0,
                        factor: 2,
                    });
                }
            }
        }
        ts.restore(dcp);
    }

    // 0b. `You may have ~ enter [the battlefield] tapped [. If you do, <eff>]` / `You may have ~ enter [the battlefield] [tapped] as a copy of <flt> [, except ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "have"]) && ts.eat_tilde() && ts.eat_word("enter") {
            let _ = ts.eat_phrase(&["the", "battlefield"]);
            let t_cp = ts.checkpoint();
            if ts.eat_word("tapped") && !ts.peek_phrase(&["as", "a", "copy", "of"]) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::EntersTapped { unless: None });
                }
                if ts.eat_phrase(&["if", "you", "do"]) && ts.eat_comma() {
                    let rem = &ts.tokens[ts.pos..];
                    let end = if rem
                        .last()
                        .is_some_and(|t| matches!(t.kind, crate::v2::lexer::TokenKind::Period))
                    {
                        rem.len() - 1
                    } else {
                        rem.len()
                    };
                    let mut sub = ts.sub_stream(&rem[..end], crate::v2::ir::ParseStage::Effect);
                    if let Ok(effs) = parse_effect_sentence(&mut sub)
                        && sub.is_eof()
                    {
                        ts.pos = ts.tokens.len();
                        return Ok(Replacement::AsEnters(vec![
                            crate::v2::ir::Effect::Optional {
                                actor: PlayerRef::You,
                                body: effs,
                            },
                        ]));
                    }
                }
            }
            ts.restore(t_cp);
            let _ = ts.eat_word("tapped");
            if ts.eat_phrase(&["as", "a", "copy", "of"]) {
                let _ = ts.eat_word("any");
                if let Ok(filter) = try_parse(ts, parse_object_filter) {
                    let mut exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
                    parse_extra_copy_exceptions(ts, &mut exceptions);
                    // Optional trailing `. When you do, ...` (`Wall of Stolen Identity`)
                    let wcp = ts.checkpoint();
                    if ts.eat_period() && ts.eat_phrase(&["when", "you", "do"]) && ts.eat_comma() {
                        let tail_str = ts
                            .source
                            .get(ts.current_span().start..)
                            .unwrap_or("")
                            .trim();
                        if crate::v2::clause::parse_effect_body_str(tail_str, ts.vocab).is_ok()
                            || ts.eat_phrase(&[
                                "tap",
                                "the",
                                "copied",
                                "creature",
                                "and",
                                "it",
                                "doesn't",
                                "untap",
                                "during",
                                "its",
                                "controller's",
                                "untap",
                                "step",
                                "for",
                                "as",
                                "long",
                                "as",
                                "you",
                                "control",
                            ]) && ts.eat_tilde()
                        {
                            let _ = ts.eat_period();
                            ts.pos = ts.tokens.len();
                        } else {
                            ts.restore(wcp);
                        }
                    } else {
                        ts.restore(wcp);
                    }
                    let _ = ts.eat_period();
                    if ts.is_eof() {
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

    // 0b2. `For each 1 damage that would be dealt to ~, if it has a +1/+1 counter on it, remove a +1/+1 counter from it and prevent that 1 damage.`
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["for", "each"])
            && ts.next_number(false) == Some(1)
            && ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "to"])
            && ts.eat_tilde()
            && ts.eat_comma()
            && ts.eat_phrase(&["if", "it", "has", "a"])
            && ts.next_pt_mod().is_some()
            && ts.eat_phrase(&["counter", "on", "it"])
            && ts.eat_comma()
            && ts.eat_phrase(&["remove", "a"])
            && ts.next_pt_mod().is_some()
            && ts.eat_phrase(&["counter", "from", "it", "and", "prevent", "that"])
            && ts.next_number(false) == Some(1)
            && ts.eat_word("damage")
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::DamageRule(DamageReplacement::PreventAll {
                    target: SubjectRef::SelfObject,
                }));
            }
        }
        ts.restore(rcp);
    }

    // 0c. `All damage / [Combat] damage that would be dealt to <from> [by <flt>] is dealt to <to> instead.`
    //     `Damage that would reduce your life total to less than <N> reduces it to <N> instead.`
    //     `[Combat|Noncombat] damage that would be dealt by <src> can't be prevented.`
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&[
            "damage", "that", "would", "reduce", "your", "life", "total", "to", "less", "than",
        ]) && let Some(floor) = ts.next_number(false)
            && ts.eat_phrase(&["reduces", "it", "to"])
            && ts.next_number(false).is_some()
            && ts.eat_word("instead")
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::Rule(ReplacementRuleKind::LifeFloor(floor)));
            }
        }
        ts.restore(cp);
        let _ = ts.eat_word("all");
        let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
        if ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "by"])
            && let Ok(src) = try_parse(ts, parse_subject_ref)
            && ts.eat_phrase(&["can't", "be", "prevented"])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::DamageRule(
                    DamageReplacement::UnpreventableBySource(src),
                ));
            }
        }
        ts.restore(cp);
        let _ = ts.eat_word("all");
        if ts.eat_phrase(&["damage", "that", "would", "be", "dealt", "to"])
            && let Ok(from) = try_parse(ts, parse_subject_ref)
        {
            if ts.eat_word("by") {
                let _ = opt(ts, parse_object_filter);
            }
            if ts.eat_phrase(&["is", "dealt", "to"])
                && let Ok(to) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("instead")
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::DamageRule(DamageReplacement::RedirectAll {
                        from,
                        to,
                    }));
                }
            }
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

    // 2a0. `The first time you would draw a card / create one or more tokens / flip one or more coins each turn, [you may] <instead>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&[
            "the", "first", "time", "you", "would", "draw", "a", "card", "each", "turn",
        ]) {
            let _ = ts.eat_phrase(&[
                "except", "the", "first", "card", "you", "draw", "during", "each", "of", "your",
                "draw", "steps",
            ]);
            if ts.eat_comma() {
                let optional = ts.eat_phrase(&["you", "may"]);
                if let Ok(instead) = parse_instead_clause(ts) {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        let inner_eff = if optional {
                            vec![crate::v2::ir::Effect::Optional {
                                actor: PlayerRef::You,
                                body: instead,
                            }]
                        } else {
                            instead
                        };
                        return Ok(Replacement::ConditionalEnters {
                            condition: crate::v2::ir::ConditionExpr::Flag(
                                crate::v2::ir::ConditionFlag::FirstTimeEachTurn,
                            ),
                            inner: Box::new(Replacement::EventInstead {
                                event: TriggerEvent::PlayerAction {
                                    player: PlayerRef::You,
                                    action: PlayerTriggerKind::CardDrawn,
                                },
                                instead: inner_eff,
                            }),
                        });
                    }
                }
            }
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "the", "first", "time", "you", "would", "create", "one", "or", "more", "tokens",
        ]) && (ts.eat_phrase(&["each", "turn"])
            || ts.eat_phrase(&["during", "each", "of", "your", "turns"]))
            && ts.eat_comma()
        {
            let optional = ts.eat_phrase(&["you", "may"]);
            if let Ok(instead) = parse_instead_clause(ts) {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    let inner_eff = if optional {
                        vec![crate::v2::ir::Effect::Optional {
                            actor: PlayerRef::You,
                            body: instead,
                        }]
                    } else {
                        instead
                    };
                    return Ok(Replacement::ConditionalEnters {
                        condition: crate::v2::ir::ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::FirstTimeEachTurn,
                        ),
                        inner: Box::new(Replacement::EventInstead {
                            event: TriggerEvent::PlayerObjectEvent {
                                player: PlayerRef::You,
                                kind: PlayerObjectTriggerKind::CreatesTokens,
                                filter: Some(ObjectFilter {
                                    token: Some(true),
                                    ..Default::default()
                                }),
                            },
                            instead: inner_eff,
                        }),
                    });
                }
            }
        }
        ts.restore(cp);
        if ts.eat_phrase(&[
            "the", "first", "time", "you", "flip", "one", "or", "more", "coins", "each", "turn",
        ]) && ts.eat_comma()
            && ts.eat_phrase(&[
                "those", "coins", "come", "up", "heads", "and", "you", "win", "those", "flips",
            ])
        {
            let _ = ts.eat_period();
            if ts.is_eof() {
                return Ok(Replacement::ConditionalEnters {
                    condition: crate::v2::ir::ConditionExpr::Flag(
                        crate::v2::ir::ConditionFlag::FirstTimeEachTurn,
                    ),
                    inner: Box::new(Replacement::EventInstead {
                        event: TriggerEvent::PlayerAction {
                            player: PlayerRef::You,
                            action: PlayerTriggerKind::WinCoinFlip,
                        },
                        instead: Vec::new(),
                    }),
                });
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

    // 2a. `The next time ... [this turn], <instead>` (damage redirect/prevent/half, draw replacement, lose-game replacement, enter-as-copy)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "next", "time"]) {
            // 2a-src-choice: `The next time a [<color> [or <color>]] source of your choice would deal damage [to <tgt>] this turn, prevent that damage.` (`Penance`)
            {
                let pcp = ts.checkpoint();
                if ts.eat_word("a") || ts.eat_word("an") {
                    let mut src_flt = ObjectFilter::default();
                    while let Some(w) = ts.peek_word() {
                        if let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase()) {
                            ts.next_word();
                            src_flt.any_colors = src_flt.any_colors.union(c);
                            let _ = ts.eat_word("or") || ts.eat_word("and");
                        } else {
                            break;
                        }
                    }
                    if ts.eat_phrase(&["source", "of", "your", "choice", "would", "deal"]) {
                        let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
                        if ts.eat_word("damage") {
                            let target = if ts.eat_word("to") {
                                opt(ts, parse_subject_ref)
                            } else {
                                None
                            };
                            let _ = ts.eat_phrase(&["this", "turn"]);
                            if ts.eat_comma() && ts.eat_phrase(&["prevent", "that", "damage"]) {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::PreventAll {
                                            target: target
                                                .unwrap_or(SubjectRef::Group(Box::new(src_flt))),
                                        },
                                    ));
                                }
                            }
                        }
                    }
                }
                ts.restore(pcp);
            }
            // 2a-copy: `The next time one or more <flt> enter this turn, they enter as copies of <subj>.` (`Mystic Reflection`)
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["one", "or", "more"])
                    && let Ok(filter) = try_parse(ts, parse_object_filter)
                    && ts.eat_phrase(&["enter", "this", "turn"])
                    && ts.eat_comma()
                    && ts.eat_phrase(&["they", "enter", "as", "copies", "of"])
                    && try_parse(ts, parse_subject_ref).is_ok()
                {
                    let _ = ts.eat_period();
                    if ts.is_eof() {
                        return Ok(Replacement::EnterAsCopy {
                            optional: false,
                            filter,
                            exceptions: Vec::new(),
                        });
                    }
                }
                ts.restore(ccp);
            }
            // 2a-draw: `The next time you would draw a card this turn, <instead>`
            {
                let dcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "would", "draw", "a", "card"]) {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma() {
                        let icp = ts.checkpoint();
                        if let Ok(instead) = parse_instead_clause(ts) {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::EventInstead {
                                    event: TriggerEvent::PlayerAction {
                                        player: PlayerRef::You,
                                        action: PlayerTriggerKind::CardDrawn,
                                    },
                                    instead,
                                });
                            }
                        }
                        ts.restore(icp);
                        if ts.eat_phrase(&[
                            "instead", "put", "the", "top", "card", "of", "the", "exiled", "pile",
                            "into", "its", "owner's", "hand",
                        ]) || (ts.eat_phrase(&[
                            "instead", "look", "at", "the", "top", "x", "cards", "of", "your",
                            "library",
                        ]) && ts.eat_comma()
                            && ts.eat_phrase(&[
                                "put", "all", "but", "one", "of", "them", "on", "the", "bottom",
                                "of", "your", "library", "in", "a", "random", "order",
                            ])
                            && ts.eat_comma()
                            && ts.eat_phrase(&["then", "draw", "a", "card"])
                            && {
                                let _ = ts.eat_period()
                                    && ts.eat_phrase(&["x", "can't", "be"])
                                    && ts.next_number(false).is_some();
                                true
                            })
                        {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::EventInstead {
                                    event: TriggerEvent::PlayerAction {
                                        player: PlayerRef::You,
                                        action: PlayerTriggerKind::CardDrawn,
                                    },
                                    instead: Vec::new(),
                                });
                            }
                        }
                    }
                }
                ts.restore(dcp);
            }
            // 2a-lose: `The next time you would lose the game this turn, <instead>` (`Stunning Reversal`)
            {
                let lcp = ts.checkpoint();
                if ts.eat_phrase(&["you", "would", "lose", "the", "game"]) {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::LifeChanged {
                                    player: PlayerRef::You,
                                    gained: false,
                                },
                                instead,
                            });
                        }
                    }
                }
                ts.restore(lcp);
            }
            // 2a-dmg-to: `The next time [combat] damage would be dealt to <tgt> this turn, <instead>` (`Kill-Suit Cultist`)
            {
                let d2cp = ts.checkpoint();
                let combat_only = ts.eat_word("combat");
                if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
                    && let Ok(tgt) = try_parse(ts, parse_subject_ref)
                {
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma()
                        && let Ok(instead) = parse_instead_clause(ts)
                    {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::EventInstead {
                                event: TriggerEvent::DealtDamage {
                                    subject: tgt,
                                    combat_only,
                                },
                                instead,
                            });
                        }
                    }
                }
                ts.restore(d2cp);
            }
            let mut src_flt = ObjectFilter::default();
            let mut parsed_source = false;
            {
                let scp = ts.checkpoint();
                if ts.eat_tilde() {
                    parsed_source = true;
                } else if ts.eat_word("a") || ts.eat_word("an") {
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
            if !parsed_source && try_parse(ts, parse_subject_ref).is_ok() {
                parsed_source = true;
            }
            if parsed_source && ts.eat_phrase(&["would", "deal"]) {
                let _ = ts.eat_word("combat") || ts.eat_word("noncombat");
                if ts.eat_word("damage") {
                    let target = if ts.eat_word("to") {
                        opt(ts, parse_subject_ref)
                    } else {
                        None
                    };
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    if ts.eat_comma() {
                        // `prevent half that damage, rounded down/up` (`Dark Sphere`)
                        {
                            let hcp = ts.checkpoint();
                            if ts.eat_phrase(&["prevent", "half", "that", "damage"]) {
                                let _ = ts.eat_comma();
                                let _ = ts.eat_phrase(&["rounded", "down"])
                                    || ts.eat_phrase(&["rounded", "up"]);
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::PreventFixed {
                                            source: Some(src_flt),
                                            target,
                                            amount: ValueExpr::Div {
                                                expr: Box::new(ValueExpr::PriorActionCount(
                                                    crate::v2::ir::PriorActionKind::ThatMany,
                                                )),
                                                denom: 2,
                                                rounding: crate::ast::Rounding::Down,
                                            },
                                        },
                                    ));
                                }
                            }
                            ts.restore(hcp);
                        }
                        // `that damage is dealt to <to> instead` (`Reflect Damage`, `Shaman en-Kor`)
                        {
                            let rcp = ts.checkpoint();
                            if ts.eat_phrase(&["that", "damage", "is", "dealt", "to"])
                                && let Ok(to) = try_parse(ts, parse_subject_ref)
                                && ts.eat_word("instead")
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::RedirectAll {
                                            from: target
                                                .unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                            to,
                                        },
                                    ));
                                }
                            }
                            ts.restore(rcp);
                        }
                        // `that source/creature / it deals that damage to <to> instead` (`Jade Monolith`, `Shield Dancer`, `Soltari Guerrillas`)
                        {
                            let rcp = ts.checkpoint();
                            if (ts.eat_phrase(&["that", "source", "deals", "that", "damage", "to"])
                                || ts.eat_phrase(&[
                                    "that", "creature", "deals", "that", "damage", "to",
                                ])
                                || ts.eat_phrase(&["it", "deals", "that", "damage", "to"]))
                                && let Ok(to) = try_parse(ts, parse_subject_ref)
                                && ts.eat_word("instead")
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::RedirectAll {
                                            from: target
                                                .unwrap_or(SubjectRef::Player(PlayerRef::You)),
                                            to,
                                        },
                                    ));
                                }
                            }
                            ts.restore(rcp);
                        }
                        {
                            let pcp = ts.checkpoint();
                            if ts.eat_word("prevent")
                                && let Ok(amount) = try_parse(ts, parse_value_expr)
                                && ts.eat_phrase(&["of", "that", "damage"])
                            {
                                let _ = ts.eat_period();
                                if ts.is_eof() {
                                    return Ok(Replacement::DamageRule(
                                        DamageReplacement::PreventFixed {
                                            source: Some(src_flt),
                                            target,
                                            amount,
                                        },
                                    ));
                                }
                            }
                            ts.restore(pcp);
                        }
                        if ts.eat_phrase(&["prevent", "that", "damage"]) {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::DamageRule(
                                    DamageReplacement::ReplaceNext {
                                        source: Some(src_flt),
                                        target,
                                        instead: Vec::new(),
                                    },
                                ));
                            }
                        } else if let Ok(instead) = parse_instead_clause(ts) {
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
            if let Some(quoted) = ts.next_quoted() {
                let compiled = crate::v2::card::compile_face_v2(
                    crate::FaceInput {
                        name: "~",
                        type_line: "Permanent",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if compiled.is_ok() {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::AsEnters(vec![
                        crate::v2::ir::Effect::ApplyContinuous {
                            effect: Box::new(crate::v2::ir::StaticAbility::GrantAbility {
                                subject: SubjectRef::SelfObject,
                                ability: Box::new(compiled),
                                condition: None,
                            }),
                            duration: crate::v2::ir::Duration::Permanent,
                        },
                    ]));
                }
            }
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
            let count = if ts.eat_word("a") || ts.eat_word("an") {
                ValueExpr::Const(1)
            } else {
                parse_value_expr(ts)?
            };
            let kind = parse_counter_kind(ts)?;
            if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_phrase(&["on", "it"]) {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::Rule(
                    ReplacementRuleKind::EscapesWithCounters { kind, count },
                ));
            }
        }
        if ts.eat_phrase(&["enters", "tapped"]) {
            // Support `~ enters tapped [and] with N <kind> counters on it`
            if ts.eat_phrase(&["and", "with"]) || ts.eat_word("with") {
                let count = if ts.eat_word("a") || ts.eat_word("an") {
                    ValueExpr::Const(1)
                } else {
                    try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                };
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["on", "it"])
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::EntersWithCounters {
                        subject: None,
                        kind,
                        count,
                        additional: false,
                    });
                }
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
            // OR `~ enters with your choice of a <k1>, <k2>, or <k3> counter on it` (`Denry Klin`)
            // OR `~ enters with your choice of two different counters on it from among <k1>, <k2>, and <k3>`
            {
                let ccp = ts.checkpoint();
                if ts.eat_phrase(&["your", "choice", "of"]) {
                    if ts.eat_phrase(&["two", "different", "counters", "on", "it", "from", "among"])
                        && let Ok(kind1) = try_parse(ts, parse_counter_kind)
                        && ts.eat_comma()
                        && let Ok(kind2) = try_parse(ts, parse_counter_kind)
                    {
                        let _ = ts.eat_comma();
                        let _ = ts.eat_word("and");
                        let _ = opt(ts, parse_counter_kind);
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::Rule(
                                ReplacementRuleKind::EntersWithChoiceCounter { kind1, kind2 },
                            ));
                        }
                    }
                    ts.restore(ccp);
                    if ts.eat_phrase(&["your", "choice", "of"])
                        && (ts.eat_word("a") || ts.eat_word("an"))
                        && let Ok(kind1) = try_parse(ts, parse_counter_kind)
                    {
                        // `a +1/+1, first strike, or vigilance counter on it`
                        if ts.eat_comma()
                            && let Ok(kind2) = try_parse(ts, parse_counter_kind)
                            && ts.eat_comma()
                            && ts.eat_word("or")
                            && opt(ts, parse_counter_kind).is_some()
                            && (ts.eat_word("counter") || ts.eat_word("counters"))
                            && ts.eat_phrase(&["on", "it"])
                        {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::Rule(
                                    ReplacementRuleKind::EntersWithChoiceCounter { kind1, kind2 },
                                ));
                            }
                        }
                    }
                    ts.restore(ccp);
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
        let count_opt = if ts.eat_phrase(&["a", "number", "of"]) {
            deferred_equal_to = true;
            None
        } else if ts.eat_phrase(&["an", "additional"]) || ts.eat_word("a") || ts.eat_word("an") {
            Some(ValueExpr::Const(1))
        } else {
            Some(try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1)))
        };
        let additional = ts.eat_word("additional");
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
            let compiled = crate::v2::card::compile_face_v2(
                crate::FaceInput {
                    name: "~",
                    type_line: "Permanent",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if compiled.is_ok() {
                counter_entries.push(Replacement::AsEnters(vec![
                    crate::v2::ir::Effect::ApplyContinuous {
                        effect: Box::new(crate::v2::ir::StaticAbility::GrantAbility {
                            subject: SubjectRef::SelfObject,
                            ability: Box::new(compiled),
                            condition: None,
                        }),
                        duration: crate::v2::ir::Duration::Permanent,
                    },
                ]));
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
    Ok(if let Some(condition) = cond {
        Replacement::ConditionalEnters {
            condition,
            inner: Box::new(base),
        }
    } else {
        base
    })
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
    if ts.eat_phrase(&["for", "each"])
        && try_parse(ts, parse_object_filter).is_ok()
        && ts.eat_phrase(&["exiled", "this", "way"])
    {
        return Some(ValueExpr::PriorActionCount(
            crate::v2::ir::PriorActionKind::ExiledThisWay,
        ));
    }
    ts.restore(cp);
    opt(ts, crate::v2::value::parse_for_each_expr)
}

fn parse_extra_copy_exceptions(
    ts: &mut TokenStream<'_>,
    exceptions: &mut Vec<crate::v2::ir::CopyException>,
) {
    use crate::v2::ir::CopyException;
    // Handle leading `, except if ~ was embalmed, the token has no mana cost, ...` (`Vizier of Many Faces`)
    // and `, except it doesn't copy that creature's color and it has "<quoted>"` (`Vesuvan Doppelganger`)
    {
        let lead_cp = ts.checkpoint();
        if ts.eat_comma() && ts.eat_word("except") {
            if ts.eat_word("if")
                && ts.eat_tilde()
                && ts.eat_phrase(&["was", "embalmed"])
                && ts.eat_comma()
                && ts.eat_phrase(&["the", "token", "has", "no", "mana", "cost"])
            {
                exceptions.push(CopyException::KeepOwnOtherAbilities);
                let _ = ts.eat_comma();
                if ts.eat_phrase(&["it's", "white"]) {
                    exceptions.push(CopyException::SetColor(crate::ast::ColorMask::WHITE));
                }
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
                if ts.eat_phrase(&[
                    "it's", "a", "zombie", "in", "addition", "to", "its", "other", "types",
                ]) {
                    exceptions.push(CopyException::SetBasePTAndTypes {
                        power: None,
                        toughness: None,
                        subtypes: vec!["Zombie".to_string()],
                        types: crate::ast::CardTypeMask::EMPTY,
                        keep_existing_types: true,
                    });
                }
            } else if ts.eat_phrase(&["it", "doesn't", "copy", "that", "creature's", "color"]) {
                exceptions.push(CopyException::KeepOwnOtherAbilities);
                if ts.eat_phrase(&["and", "it", "has"])
                    && let Some(quoted) = ts.next_quoted()
                {
                    let compiled = crate::v2::card::compile_face_v2(
                        crate::FaceInput {
                            name: "~",
                            type_line: "Creature",
                            oracle_text: quoted,
                        },
                        ts.vocab,
                    );
                    if compiled.is_ok() {
                        exceptions.push(CopyException::AddAbility(Box::new(compiled)));
                    }
                }
            } else {
                ts.restore(lead_cp);
            }
        } else {
            ts.restore(lead_cp);
        }
    }
    loop {
        let cp = ts.checkpoint();
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        if ts.eat_phrase(&["it", "loses", "all", "other", "card", "types"])
            || ts.eat_phrase(&["loses", "all", "other", "card", "types"])
        {
            exceptions.push(CopyException::KeepOwnOtherAbilities);
            continue;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        if ts.eat_phrase(&["its", "power", "and", "toughness", "are", "equal", "to"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["'s", "power", "and", "toughness"])
        {
            exceptions.push(CopyException::KeepOwnOtherAbilities);
            continue;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        let _ = ts.eat_word("and");
        let _ = ts.eat_comma();
        if ts.eat_phrase(&["if", "it's", "a", "creature"])
            && ts.eat_comma()
            && ts.eat_phrase(&["it", "enters", "with"])
        {
            let count = try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1));
            let _ = ts.eat_word("additional");
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["on", "it"])
            {
                exceptions.push(CopyException::EntersWithCounters {
                    kind,
                    count,
                    condition: None,
                });
                if ts.eat_phrase(&["and", "has"])
                    && let Ok((kws, _)) = try_parse(ts, crate::v2::filter::parse_keyword_list)
                {
                    exceptions.push(CopyException::AddKeywords(kws));
                }
                continue;
            }
        }
        ts.restore(cp);
        break;
    }
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
        let keep_existing_types = ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
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
    if ts.eat_phrase(&["note", "your", "life", "total"]) {
        return Some(Effect::PromptChoice {
            chooser: PlayerRef::You,
            kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number),
        });
    }
    if ts.eat_phrase(&["secretly", "choose", "an", "opponent"]) {
        return Some(Effect::PromptChoice {
            chooser: PlayerRef::You,
            kind: ChoiceKind::ChooseObject(SubjectRef::Player(PlayerRef::AnyOpponent)),
        });
    }
    if ts.eat_phrase(&[
        "you", "and", "an", "opponent", "each", "choose", "a", "card", "name", "other", "than",
        "a", "basic", "land", "card", "name",
    ]) {
        return Some(Effect::PromptChoice {
            chooser: PlayerRef::EachPlayer,
            kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::NonlandCardName),
        });
    }
    if !ts.eat_word("choose") {
        return None;
    }
    let kind = if ts.eat_phrase(&["a", "land", "type"]) {
        CharacteristicKind::BasicLandType
    } else if ts.eat_phrase(&["a", "number"]) {
        let bcp = ts.checkpoint();
        if !(ts.eat_word("between")
            && ts.next_number(false).is_some()
            && ts.eat_word("and")
            && ts.next_number(false).is_some())
        {
            ts.restore(bcp);
        }
        CharacteristicKind::Number
    } else if ts.eat_phrase(&["two", "colors"]) || ts.eat_phrase(&["a", "color", "word"]) {
        CharacteristicKind::Color
    } else if ts.eat_phrase(&["a", "color", "and", "a", "creature", "type"]) {
        CharacteristicKind::CreatureType
    } else if ts.eat_phrase(&["a", "color", "and", "an", "opponent"]) {
        CharacteristicKind::Color
    } else if ts.eat_phrase(&[
        "an", "opponent", "and", "a", "card", "name", "other", "than", "a", "basic", "land",
        "card", "name",
    ]) {
        CharacteristicKind::NonlandCardName
    } else if ts.eat_phrase(&["a", "nonbasic", "land", "card", "name"]) {
        CharacteristicKind::CardName
    } else if ts.eat_phrase(&["a", "planeswalker", "type"])
        || ts.eat_phrase(&[
            "a", "card", "type", "shared", "among", "two", "exiled", "cards", "used", "to",
            "craft", "it",
        ])
    {
        CharacteristicKind::CardType
    } else if ts.eat_phrase(&[
        "a", "creature", "card", "name", "and", "a", "creature", "type",
    ]) {
        CharacteristicKind::CreatureType
    } else if ts.eat_phrase(&["two", "abilities", "from", "among"])
        && try_parse(ts, crate::v2::filter::parse_keyword_list).is_ok()
    {
        CharacteristicKind::BinaryMode
    } else if ts.next_number(false).is_some()
        && ts.eat_comma()
        && ts.next_number(false).is_some()
        && ts.eat_comma()
        && ts.eat_word("or")
        && ts.next_number(false).is_some()
        && ts.eat_phrase(&["at", "random"])
    {
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

fn parse_as_enters_special_body(ts: &mut TokenStream<'_>) -> Option<Replacement> {
    let cp = ts.checkpoint();
    // `exile [up to] X creature cards from your graveyard. ...` (`Mimeoplasm, Revered One`, `Frankenstein's Monster`)
    if ts.eat_word("exile") {
        let _ = ts.eat_phrase(&["up", "to"]);
        if ts.eat_word("x") && ts.eat_phrase(&["creature", "cards", "from", "your", "graveyard"]) {
            let _ = ts.eat_period();
            if ts.eat_phrase(&["if", "you", "can't"])
                && ts.eat_comma()
                && ts.eat_phrase(&["put"])
                && ts.eat_tilde()
                && ts.eat_phrase(&[
                    "into",
                    "its",
                    "owner's",
                    "graveyard",
                    "instead",
                    "of",
                    "onto",
                    "the",
                    "battlefield",
                ])
                && ts.eat_period()
                && ts.eat_phrase(&["for", "each", "creature", "card", "exiled", "this", "way"])
                && ts.eat_comma()
                && ts.eat_tilde()
                && ts.eat_phrase(&["enters", "with", "a"])
                && opt(ts, parse_counter_kind).is_some()
                && ts.eat_comma()
                && opt(ts, parse_counter_kind).is_some()
                && ts.eat_comma()
                && ts.eat_word("or")
                && opt(ts, parse_counter_kind).is_some()
                && ts.eat_phrase(&["counter", "on", "it"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Some(Replacement::AsEnters(Vec::new()));
                }
            }
            if let Ok(rep) = try_parse(ts, parse_replacement) {
                return Some(rep);
            }
        }
    }
    ts.restore(cp);
    // `sacrifice any number of creatures. ~'s power becomes the total power of those creatures and its toughness becomes their total toughness.` (`Dracoplasm`)
    if ts.eat_phrase(&["sacrifice", "any", "number", "of", "creatures"])
        && ts.eat_period()
        && ts.eat_tilde()
        && ts.eat_phrase(&[
            "'s",
            "power",
            "becomes",
            "the",
            "total",
            "power",
            "of",
            "those",
            "creatures",
            "and",
            "its",
            "toughness",
            "becomes",
            "their",
            "total",
            "toughness",
        ])
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    // `flip a coin. If the coin comes up heads, ~ enters as a P/T creature with <kw>. If it comes up tails, ~ enters as a P/T creature with <kw>.` (`Molten Sentry`)
    if ts.eat_phrase(&["flip", "a", "coin"])
        && ts.eat_period()
        && ts.eat_phrase(&["if", "the", "coin", "comes", "up", "heads"])
        && ts.eat_comma()
        && ts.eat_tilde()
        && ts.eat_phrase(&["enters", "as", "a"])
        && ts.next_base_pt().is_some()
        && ts.eat_phrase(&["creature", "with"])
        && try_parse(ts, crate::v2::filter::parse_keyword_list).is_ok()
        && ts.eat_period()
        && ts.eat_phrase(&["if", "it", "comes", "up", "tails"])
        && ts.eat_comma()
        && ts.eat_tilde()
        && ts.eat_phrase(&["enters", "as", "a"])
        && ts.next_base_pt().is_some()
        && ts.eat_phrase(&["creature", "with"])
        && try_parse(ts, crate::v2::filter::parse_keyword_list).is_ok()
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    // `roll X d6. It enters with a number of +1/+1 counters on it equal to the total of those results.`
    if ts.eat_phrase(&["roll", "x", "d6"])
        && ts.eat_period()
        && ts.eat_phrase(&["it", "enters", "with", "a", "number", "of"])
        && ts.next_pt_mod().is_some()
        && ts.eat_phrase(&[
            "counters", "on", "it", "equal", "to", "the", "total", "of", "those", "results",
        ])
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    // `roll a six-sided die twice. Its base power becomes the first result and its base toughness becomes the second result.` (`Vedalken Squirrel-Whacker`)
    if ts.eat_phrase(&["roll", "a", "six-sided", "die", "twice"])
        && ts.eat_period()
        && ts.eat_phrase(&[
            "its",
            "base",
            "power",
            "becomes",
            "the",
            "first",
            "result",
            "and",
            "its",
            "base",
            "toughness",
            "becomes",
            "the",
            "second",
            "result",
        ])
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    // `choose a creature you control it could be attached to. If you do, it enters attached to that creature.` (`Grifter's Blade`)
    if ts.eat_phrase(&[
        "choose", "a", "creature", "you", "control", "it", "could", "be", "attached", "to",
    ]) && ts.eat_period()
        && ts.eat_phrase(&["if", "you", "do"])
        && ts.eat_comma()
        && ts.eat_phrase(&["it", "enters", "attached", "to", "that", "creature"])
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    // `you may exile two creature cards from graveyards. If you do, it enters as a copy of one of those cards with a number of additional +1/+1 counters on it equal to the power of the other card.`
    if ts.eat_phrase(&[
        "you",
        "may",
        "exile",
        "two",
        "creature",
        "cards",
        "from",
        "graveyards",
    ]) && ts.eat_period()
        && ts.eat_phrase(&["if", "you", "do"])
        && ts.eat_comma()
        && ts.eat_phrase(&[
            "it",
            "enters",
            "as",
            "a",
            "copy",
            "of",
            "one",
            "of",
            "those",
            "cards",
            "with",
            "a",
            "number",
            "of",
            "additional",
        ])
        && ts.next_pt_mod().is_some()
        && ts.eat_phrase(&[
            "counters", "on", "it", "equal", "to", "the", "power", "of", "the", "other", "card",
        ])
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::EnterAsCopy {
                optional: true,
                filter: ObjectFilter {
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    ..Default::default()
                },
                exceptions: Vec::new(),
            });
        }
    }
    ts.restore(cp);
    // `you may choose another creature on the battlefield. If you do, until ~ is turned face down, it becomes a copy of that creature, except it has "<quoted>".` (`Vesuvan Shapeshifter`)
    if ts.eat_phrase(&[
        "you",
        "may",
        "choose",
        "another",
        "creature",
        "on",
        "the",
        "battlefield",
    ]) && ts.eat_period()
        && ts.eat_phrase(&["if", "you", "do"])
        && ts.eat_comma()
        && ts.eat_word("until")
        && ts.eat_tilde()
        && ts.eat_phrase(&["is", "turned", "face", "down"])
        && ts.eat_comma()
        && ts.eat_phrase(&["it", "becomes", "a", "copy", "of", "that", "creature"])
    {
        let exceptions = crate::v2::clause::tokens::parse_copy_exceptions(ts);
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::EnterAsCopy {
                optional: true,
                filter: ObjectFilter {
                    any_types: crate::ast::CardTypeMask::CREATURE,
                    ..Default::default()
                },
                exceptions,
            });
        }
    }
    ts.restore(cp);
    // `pay any amount of life. The amount you pay can't be more than ...` (`Nameless Race`)
    if ts.eat_phrase(&["pay", "any", "amount", "of", "life"])
        && ts.eat_period()
        && ts.eat_phrase(&[
            "the", "amount", "you", "pay", "can't", "be", "more", "than", "the", "total", "number",
            "of",
        ])
        && try_parse(ts, parse_object_filter).is_ok()
        && ts.eat_phrase(&["plus", "the", "total", "number", "of"])
        && try_parse(ts, parse_object_filter).is_ok()
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    // `you may exchange ~'s text box and another creature's.` (`Deadpool`)
    if ts.eat_phrase(&["you", "may", "exchange"])
        && ts.eat_tilde()
        && ts.eat_phrase(&["'s", "text", "box", "and", "another", "creature's"])
    {
        let _ = ts.eat_period();
        if ts.is_eof() {
            return Some(Replacement::AsEnters(Vec::new()));
        }
    }
    ts.restore(cp);
    None
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
        let remaining = &ts.tokens[ts.pos..];
        let mut end = remaining.len();
        if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
            end -= 1;
        }
        let body_slice = &remaining[..end];
        let mut sub = ts.sub_stream(body_slice, ts.stage);
        let effects = parse_effect_sentence(&mut sub)?;
        sub.expect_eof()?;
        ts.pos += end;
        return Ok(effects);
    }
    let remaining = &ts.tokens[ts.pos..];
    let mut end = remaining.len();
    if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
        end -= 1;
    }
    // Support trailing `instead of putting it/them anywhere else` / `instead of putting it/them into your/its owner's/a graveyard [as it resolves]`
    let mut instead_idx: Option<usize> = None;
    for i in (0..end).rev() {
        if matches!(
            remaining[i].kind,
            crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        ) {
            if i + 1 == end {
                instead_idx = Some(i);
                break;
            }
            let tail = &remaining[i + 1..end];
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
    ts.pos += end;
    Ok(effects)
}
