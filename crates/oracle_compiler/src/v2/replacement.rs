//! Strictly-typed [`Replacement`] parser for `v2`.

use crate::v2::clause::parse_effect_sentence;
use crate::v2::condition::parse_condition;
use crate::v2::filter::{parse_counter_kind, parse_object_filter};
use crate::v2::ir::{PlayerRef, Replacement, SubjectRef, TriggerEvent, ValueExpr};
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a strictly-typed [`Replacement`] sentence (`As ~ enters, ...`,
/// `If <event> would ..., <instead> instead`,
/// `The next <flt> you cast this turn enters with ...`).
pub fn parse_replacement(ts: &mut TokenStream<'_>) -> PResult<Replacement> {
    // 0. `As ~ enters, <body>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("as") && ts.eat_tilde() && ts.eat_word("enters") && ts.eat_comma() {
            let body = parse_effect_sentence(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::AsEnters(body));
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

        let flt = parse_object_filter(ts)?;
        if ts.eat_phrase(&["would", "die"]) && ts.eat_comma() {
            let instead = parse_instead_clause(ts)?;
            let _ = ts.eat_period();
            ts.expect_eof()?;
            return Ok(Replacement::EventInstead {
                event: TriggerEvent::Dies {
                    subject: SubjectRef::Group(Box::new(flt)),
                },
                instead,
            });
        }
        return Err(ts.err("unsupported `If ... would ...` replacement event"));
    }

    // 2. `The next <flt> you cast this turn enters with [a number of] [additional] <kind> counters on it equal to <val>`
    if ts.eat_phrase(&["the", "next"]) {
        let flt = parse_object_filter(ts)?;
        if ts.eat_phrase(&["you", "cast", "this", "turn", "enters", "with"]) {
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
