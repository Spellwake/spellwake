//! Strictly-typed [`Replacement`] parser for `v2`.

use crate::v2::clause::parse_effect_sentence;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter, parse_subject_ref};
use crate::v2::ir::{ObjectFilter, PlayerRef, Replacement, SubjectRef, TriggerEvent, ValueExpr};
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed [`Replacement`] sentence (`As ~ enters, ...`,
/// `If <event> would ..., <instead> instead`,
/// `The next <flt> you cast this turn enters with ...`).
pub fn parse_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 0. `As ~ enters, <body (one or more sentences)>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("as") && ts.eat_tilde() && ts.eat_word("enters") && ts.eat_comma() {
            let mut body = Vec::new();
            let mut cur_start = ts.pos;
            for idx in ts.pos..ts.tokens.len() {
                if matches!(ts.tokens[idx].kind, crate::v2::lexer::TokenKind::Period) {
                    if idx > cur_start {
                        let mut sub = ts.sub_stream(
                            &ts.tokens[cur_start..idx],
                            crate::v2::ir::ParseStage::Effect,
                        );
                        body.extend(parse_effect_sentence(&mut sub)?);
                        sub.expect_eof()?;
                    }
                    cur_start = idx + 1;
                }
            }
            if cur_start < ts.tokens.len() {
                let mut sub =
                    ts.sub_stream(&ts.tokens[cur_start..], crate::v2::ir::ParseStage::Effect);
                body.extend(parse_effect_sentence(&mut sub)?);
                sub.expect_eof()?;
            }
            ts.pos = ts.tokens.len();
            return Ok(Replacement::AsEnters(body));
        }
        ts.restore(cp);
    }

    // 0b. `You may have ~ enter as a copy of <flt> [, except ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["you", "may", "have"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["enter", "as", "a", "copy", "of"])
        {
            let _ = ts.eat_word("any");
            let filter = parse_object_filter(ts)?;
            let mut exceptions = Vec::new();
            let ecp = ts.checkpoint();
            if ts.eat_comma() && ts.eat_word("except") {
                if ts.eat_phrase(&["it", "has"]) {
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
                            exceptions
                                .push(crate::v2::ir::CopyException::AddAbility(Box::new(compiled)));
                        } else {
                            ts.restore(ecp);
                        }
                    } else if let Ok((kws, _)) =
                        try_parse(ts, crate::v2::filter::parse_keyword_list)
                    {
                        exceptions.push(crate::v2::ir::CopyException::AddKeywords(kws));
                    } else {
                        ts.restore(ecp);
                    }
                } else if ts.eat_phrase(&["it", "isn't", "legendary"]) {
                    exceptions.push(crate::v2::ir::CopyException::NotLegendary);
                } else if ts.eat_phrase(&["it's", "a"]) || ts.eat_phrase(&["it's", "an"]) {
                    let pt = ts.next_base_pt();
                    let mut subtypes = Vec::new();
                    let mut types = crate::ast::CardTypeMask::EMPTY;
                    while let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(ct) = crate::vocab::lookup_card_type(&low) {
                            ts.next_word();
                            types = types.union(ct);
                        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            subtypes.push(sub.to_string());
                        } else {
                            break;
                        }
                    }
                    let keep_existing_types =
                        ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
                    if pt.is_some() || !subtypes.is_empty() || !types.is_empty() {
                        exceptions.push(crate::v2::ir::CopyException::SetBasePTAndTypes {
                            power: pt.map(|(p, _)| p),
                            toughness: pt.map(|(_, t)| t),
                            subtypes,
                            types,
                            keep_existing_types,
                        });
                    } else {
                        ts.restore(ecp);
                    }
                } else {
                    ts.restore(ecp);
                }
            }
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EnterAsCopy {
                optional: true,
                filter,
                exceptions,
            });
        }
        ts.restore(cp);
    }

    // 1. `If ... would ...`
    if ts.eat_word("if") {
        // 1a. `If an effect would create one or more tokens under your control, it creates twice that many of those tokens instead.`
        if ts.eat_phrase(&[
            "an", "effect", "would", "create", "one", "or", "more", "tokens", "under", "your",
            "control",
        ]) && ts.eat_comma()
            && ts.eat_phrase(&[
                "it", "creates", "twice", "that", "many", "of", "those", "tokens", "instead",
            ])
        {
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::MultiplyTokens {
                controller: PlayerRef::You,
                factor: 2,
            });
        }
        // 1b. `If an effect would put one or more counters on <flt>, it puts twice that many of those counters on that permanent instead.`
        if ts.eat_phrase(&[
            "an", "effect", "would", "put", "one", "or", "more", "counters", "on",
        ]) {
            let flt = parse_object_filter(ts)?;
            if ts.eat_comma()
                && ts.eat_phrase(&[
                    "it",
                    "puts",
                    "twice",
                    "that",
                    "many",
                    "of",
                    "those",
                    "counters",
                    "on",
                    "that",
                    "permanent",
                    "instead",
                ])
            {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::MultiplyCounters {
                    filter: flt,
                    factor: 2,
                });
            }
            return Err(ts.err("unsupported counter-doubling replacement clause"));
        }

        // 1c. `If you would gain life, you gain that much life plus <N> instead.` / `you gain twice that much life instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["you", "would", "gain", "life"])
                && ts.eat_comma()
                && ts.eat_phrase(&["you", "gain"])
            {
                if ts.eat_phrase(&["twice", "that", "much", "life", "instead"]) {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::ModifyLifeGain {
                        player: PlayerRef::You,
                        add: 0,
                        factor: 2,
                    });
                }
                if ts.eat_phrase(&["that", "much", "life", "plus"])
                    && let Some(n) = ts.next_number(false)
                    && ts.eat_word("instead")
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::ModifyLifeGain {
                        player: PlayerRef::You,
                        add: i16::try_from(n).unwrap_or(1),
                        factor: 1,
                    });
                }
            }
            ts.restore(cp);
        }

        // 1d. `If one or more [<kind>] counters would be put on <flt>, that many plus <N> of those counters are put on it instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["one", "or", "more"]) {
                let kind = opt(ts, parse_counter_kind);
                if (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_phrase(&["would", "be", "put", "on"])
                    && let Ok(flt) = try_parse(ts, parse_object_filter)
                    && ts.eat_comma()
                    && ts.eat_phrase(&["that", "many", "plus"])
                    && let Some(n) = ts.next_number(false)
                    && (ts.eat_phrase(&[
                        "of", "those", "counters", "are", "put", "on", "it", "instead",
                    ]) || ts.eat_phrase(&["are", "put", "on", "it", "instead"])
                        || ts.eat_phrase(&[
                            "of",
                            "those",
                            "counters",
                            "are",
                            "put",
                            "on",
                            "that",
                            "permanent",
                            "instead",
                        ]))
                {
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::ModifyCountersPlaced {
                        filter: flt,
                        kind,
                        add: i16::try_from(n).unwrap_or(1),
                    });
                }
            }
            ts.restore(cp);
        }

        // 1e. `If a [color] source you control would deal [noncombat] damage to <tgt>, it deals that much damage plus <N> / double that damage instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_word("a") || ts.eat_word("an") {
                let mut src_flt = ObjectFilter::default();
                if let Some(w) = ts.peek_word()
                    && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
                {
                    ts.next_word();
                    src_flt.any_colors = src_flt.any_colors.union(c);
                }
                if ts.eat_word("source") {
                    if ts.eat_phrase(&["you", "control"]) {
                        src_flt.controller = Some(crate::ast::ControllerRelation::You);
                    }
                    if ts.eat_phrase(&["would", "deal"]) {
                        let _ = ts.eat_word("noncombat");
                        if ts.eat_word("damage") {
                            let target = if ts.eat_word("to") {
                                opt(ts, parse_subject_ref)
                            } else {
                                None
                            };
                            if ts.eat_comma() && ts.eat_phrase(&["it", "deals"]) {
                                if ts.eat_phrase(&["double", "that", "damage", "instead"])
                                    || ts
                                        .eat_phrase(&["twice", "that", "much", "damage", "instead"])
                                {
                                    let _ = ts.eat_period();
                                    ts.expect_eof()?;
                                    return Ok(Replacement::ModifyDamage {
                                        source: Some(src_flt),
                                        target,
                                        add: 0,
                                        factor: 2,
                                    });
                                }
                                if ts.eat_phrase(&["triple", "that", "damage", "instead"]) {
                                    let _ = ts.eat_period();
                                    ts.expect_eof()?;
                                    return Ok(Replacement::ModifyDamage {
                                        source: Some(src_flt),
                                        target,
                                        add: 0,
                                        factor: 3,
                                    });
                                }
                                if ts.eat_phrase(&["that", "much", "damage", "plus"])
                                    && let Some(n) = ts.next_number(false)
                                    && ts.eat_word("instead")
                                {
                                    let _ = ts.eat_period();
                                    ts.expect_eof()?;
                                    return Ok(Replacement::ModifyDamage {
                                        source: Some(src_flt),
                                        target,
                                        add: i16::try_from(n).unwrap_or(1),
                                        factor: 1,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        // 1f. `If damage would be dealt to <subj>, prevent that damage.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["damage", "would", "be", "dealt", "to"])
                && let Ok(target) = try_parse(ts, parse_subject_ref)
                && ts.eat_comma()
                && ts.eat_phrase(&["prevent", "that", "damage"])
            {
                let _ = ts.eat_period();
                if ts.is_eof() {
                    return Ok(Replacement::PreventDamageReplacement { target });
                }
            }
            ts.restore(cp);
        }

        // 1g. `If you would draw a card, <instead> instead.`
        {
            let cp = ts.checkpoint();
            if ts.eat_phrase(&["you", "would", "draw", "a", "card"])
                && ts.eat_comma()
                && let Ok(instead) = parse_instead_clause(ts)
            {
                let _ = ts.eat_period();
                ts.expect_eof()?;
                return Ok(Replacement::EventInstead {
                    event: TriggerEvent::CardDrawn {
                        player: PlayerRef::You,
                    },
                    instead,
                });
            }
            ts.restore(cp);
        }

        // 1h. `If <cond>, it/~ enters with <N> <kind> counters on it / enters tapped`
        {
            let cp = ts.checkpoint();
            if let Ok(condition) = try_parse(ts, parse_condition) {
                let _ = ts.eat_phrase(&["with", "its"])
                    && ts.eat_mana_cost().is_some()
                    && ts.eat_word("kicker");
                if ts.eat_comma() && (ts.eat_word("it") || ts.eat_tilde()) {
                    if ts.eat_phrase(&["enters", "tapped"]) {
                        let _ = ts.eat_period();
                        if ts.is_eof() {
                            return Ok(Replacement::ConditionalEnters {
                                condition,
                                inner: Box::new(Replacement::EntersTapped { unless: None }),
                            });
                        }
                    } else if ts.eat_phrase(&["enters", "with"]) {
                        let count = if ts.eat_word("a") || ts.eat_word("an") {
                            ValueExpr::Const(1)
                        } else {
                            try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1))
                        };
                        let additional = ts.eat_word("additional");
                        if let Ok(kind) = try_parse(ts, parse_counter_kind)
                            && (ts.eat_word("counters") || ts.eat_word("counter"))
                            && ts.eat_phrase(&["on", "it"])
                        {
                            let _ = ts.eat_period();
                            if ts.is_eof() {
                                return Ok(Replacement::ConditionalEnters {
                                    condition,
                                    inner: Box::new(Replacement::EntersWithCounters {
                                        subject: None,
                                        kind,
                                        count,
                                        additional,
                                    }),
                                });
                            }
                        }
                    }
                }
            }
            ts.restore(cp);
        }

        let subj = parse_subject_ref(ts)?;
        if (ts.eat_phrase(&["would", "enter", "the", "battlefield"])
            || ts.eat_phrase(&["would", "enter"]))
            && {
                let _ = ts.eat_phrase(&["this", "turn"]);
                ts.eat_comma()
            }
        {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::ZoneChange {
                    from: None,
                    to: Some(crate::v2::ir::ZoneKind::Battlefield),
                    subject: subj,
                },
                instead,
            });
        }
        if ts.eat_phrase(&["would", "die"]) && {
            let _ = ts.eat_phrase(&["this", "turn"]);
            ts.eat_comma()
        } {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::Dies { subject: subj },
                instead,
            });
        }
        if ts.eat_phrase(&["would", "leave", "the", "battlefield"]) && {
            let _ = ts.eat_phrase(&["this", "turn"]);
            ts.eat_comma()
        } {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::ZoneChange {
                    from: Some(crate::v2::ir::ZoneKind::Battlefield),
                    to: None,
                    subject: subj,
                },
                instead,
            });
        }
        if ts.eat_phrase(&["would", "be", "put", "into"]) {
            let z = crate::v2::filter::parse_zone_ref(ts)?;
            if z.zone == crate::v2::ir::ZoneKind::Graveyard {
                let from = if ts.eat_phrase(&["from", "the", "battlefield"]) {
                    Some(crate::v2::ir::ZoneKind::Battlefield)
                } else {
                    let _ = ts.eat_phrase(&["from", "anywhere"]);
                    None
                };
                let _ = ts.eat_phrase(&["this", "turn"]);
                if ts.eat_comma() {
                    let instead = parse_instead_clause(ts)?;
                    let _ = ts.eat_period();
                    ts.expect_eof()?;
                    return Ok(Replacement::EventInstead {
                        event: TriggerEvent::PutIntoGraveyard {
                            subject: subj,
                            from,
                        },
                        instead,
                    });
                }
            }
        }
        return Err(ts.err("unsupported `If ... would ...` replacement event"));
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

    // 3. `~ enters tapped [unless <cond>]` / `~ enters with N <kind> counters on it`
    if ts.eat_tilde() {
        if ts.eat_phrase(&["enters", "tapped"]) {
            let unless = if ts.eat_word("unless") {
                Some(parse_condition(ts)?)
            } else {
                None
            };
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EntersTapped { unless });
        }
        if ts.eat_phrase(&["enters", "with"]) {
            let count = try_parse(ts, parse_value_expr).unwrap_or(ValueExpr::Const(1));
            let kind = parse_counter_kind(ts)?;
            if (ts.eat_word("counters") || ts.eat_word("counter")) && ts.eat_phrase(&["on", "it"]) {
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
    }

    Err(ts.err("expected replacement effect"))
}

fn parse_instead_clause(ts: &mut TokenStream<'_>) -> PResult<Vec<crate::v2::ir::Effect>> {
    // Find trailing `instead` token in the remaining slice so we parse the inner effect cleanly
    let remaining = &ts.tokens[ts.pos..];
    let mut end = remaining.len();
    if end > 0 && matches!(remaining[end - 1].kind, crate::v2::lexer::TokenKind::Period) {
        end -= 1;
    }
    if end == 0
        || !matches!(
            remaining[end - 1].kind,
            crate::v2::lexer::TokenKind::Word(w) if w.eq_ignore_ascii_case("instead")
        )
    {
        return Err(ts.err("expected trailing `instead` on replacement clause"));
    }
    let body_slice = &remaining[..end - 1];
    let mut sub = ts.sub_stream(body_slice, ts.stage);
    let effects = parse_effect_sentence(&mut sub)?;
    sub.expect_eof()?;
    ts.pos += end;
    Ok(effects)
}
