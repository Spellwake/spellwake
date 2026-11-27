//! Strictly-typed [`Cost`] and cost-list parser for `v2`.

use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{Cost, ManaSymbol, ObjectFilter, ValueExpr};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, sep_by1, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a comma- or `and`-separated list of activation/additional costs (e.g. `{2}, {T}, Sacrifice ~`,
/// or `reveal a Dinosaur card from your hand or pay {1}`).
pub fn parse_cost_list(ts: &mut TokenStream<'_>) -> PResult<Vec<Cost>> {
    sep_by1(ts, parse_cost_or_alt, |s| {
        if s.eat_comma() {
            let _ = s.eat_word("and");
            Ok(())
        } else if s.eat_word("and") {
            Ok(())
        } else {
            Err(s.err("expected `,` or `and` between costs"))
        }
    })
}

fn parse_cost_or_alt(ts: &mut TokenStream<'_>) -> PResult<Cost> {
    let first = parse_cost_atom(ts)?;
    let cp = ts.checkpoint();
    if ts.eat_word("or")
        && let Ok(alt) = try_parse(ts, parse_cost_atom)
    {
        return Ok(Cost::Or(Box::new(first), Box::new(alt)));
    }
    ts.restore(cp);
    Ok(first)
}

/// Parse one strictly-typed [`Cost`] atom.
pub fn parse_cost_atom(ts: &mut TokenStream<'_>) -> PResult<Cost> {
    if matches!(
        ts.peek().map(|t| &t.kind),
        Some(TokenKind::Mana(ManaSymbol::Tap))
    ) {
        ts.next_tok();
        return Ok(Cost::TapSelf);
    }
    if matches!(
        ts.peek().map(|t| &t.kind),
        Some(TokenKind::Mana(ManaSymbol::Untap))
    ) {
        ts.next_tok();
        return Ok(Cost::UntapSelf);
    }
    if let Some(mana) = ts.eat_mana_cost() {
        return Ok(Cost::Mana(mana));
    }

    // `Pay <val> life` / `Pay {E}...` / `Pay <mana>`
    if ts.eat_word("pay") {
        let mut energy = 0i32;
        while matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(ManaSymbol::Energy))
        ) {
            ts.next_tok();
            energy += 1;
        }
        if energy > 0 {
            return Ok(Cost::PayEnergy(ValueExpr::Const(energy)));
        }
        if let Some(mana) = ts.eat_mana_cost() {
            return Ok(Cost::Mana(mana));
        }
        let amt = parse_value_expr(ts)?;
        if ts.eat_word("life") {
            return Ok(Cost::PayLife(amt));
        }
        if matches!(
            ts.peek().map(|t| &t.kind),
            Some(TokenKind::Mana(ManaSymbol::Energy))
        ) {
            ts.next_tok();
            return Ok(Cost::PayEnergy(amt));
        }
        return Err(ts.err("expected `life`, energy, or mana after `pay`"));
    }

    // `Sacrifice ~` / `Sacrifice <N> <filter>`
    if ts.eat_word("sacrifice") {
        if ts.eat_tilde() {
            return Ok(Cost::SacrificeSelf);
        }
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let filter = parse_object_filter(ts)?;
        return Ok(Cost::Sacrifice { count, filter });
    }

    // `Exert ~`
    if ts.eat_word("exert") && ts.eat_tilde() {
        return Ok(Cost::ExertSelf);
    }

    // `Collect evidence <N>`
    if ts.eat_phrase(&["collect", "evidence"]) {
        let amt = parse_value_expr(ts)?;
        return Ok(Cost::CollectEvidence(amt));
    }

    // `Exile ~` / `Exile the top [N] card(s) of <zone>` / `Exile <N> <filter> [from <zone>]`
    if ts.eat_word("exile") {
        if ts.eat_tilde() {
            let _ = ts.eat_word("from") && try_parse(ts, parse_zone_ref).is_ok();
            return Ok(Cost::ExileSelf);
        }
        if ts.eat_phrase(&["the", "top"]) {
            let count = if ts.eat_word("card") {
                ValueExpr::Const(1)
            } else {
                let n = parse_value_expr(ts)?;
                let _ = ts.eat_word("cards") || ts.eat_word("card");
                n
            };
            if ts.eat_word("of") {
                let zone = parse_zone_ref(ts)?;
                return Ok(Cost::ExileTop { zone, count });
            }
        }
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let filter = parse_object_filter(ts)?;
        let mut from = Vec::new();
        if ts.eat_word("from") {
            from.push(parse_zone_ref(ts)?);
        }
        return Ok(Cost::Exile {
            count,
            filter,
            from,
        });
    }

    // `Tap <N> untapped <filter>`
    if ts.eat_word("tap") {
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let filter = parse_object_filter(ts)?;
        return Ok(Cost::TapUntapped { count, filter });
    }

    // `Return <subj> to its owner's hand`
    if ts.eat_word("return") {
        let subject = parse_subject_ref(ts)?;
        if ts.eat_phrase(&["to", "its", "owner's", "hand"])
            || ts.eat_phrase(&["to", "their", "owners'", "hands"])
            || ts.eat_phrase(&["to", "your", "hand"])
        {
            return Ok(Cost::ReturnToHand { subject });
        }
        return Err(ts.err("expected `to its owner's hand` in return cost"));
    }

    // `Discard your hand` / `Discard ~` / `Discard <N> <filter> [at random]`
    if ts.eat_word("discard") {
        if ts.eat_phrase(&["your", "hand"]) {
            return Ok(Cost::DiscardHand);
        }
        if ts.eat_tilde() {
            return Ok(Cost::Discard {
                count: ValueExpr::Const(1),
                filter: Some(ObjectFilter {
                    any_card: true,
                    ..Default::default()
                }),
                random: false,
            });
        }
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let filter = if ts.eat_word("card") || ts.eat_word("cards") {
            None
        } else {
            Some(parse_object_filter(ts)?)
        };
        let random = ts.eat_phrase(&["at", "random"]);
        return Ok(Cost::Discard {
            count,
            filter,
            random,
        });
    }

    // `Remove <N | all | any number of> [<kind>] counter(s) from [among] <subj>`
    if ts.eat_word("remove") {
        let count = if ts.eat_word("all") || ts.eat_phrase(&["any", "number", "of"]) {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let kind = if ts.eat_word("counter") || ts.eat_word("counters") {
            crate::v2::ir::CounterKind::Any
        } else {
            let ck = parse_counter_kind(ts)?;
            if !(ts.eat_word("counter") || ts.eat_word("counters")) {
                return Err(ts.err("expected `counter(s)` in remove-counter cost"));
            }
            ck
        };
        if ts.eat_word("from") {
            let _ = ts.eat_word("among");
            let subject = if ts.eat_tilde() {
                None
            } else {
                Some(parse_subject_ref(ts)?)
            };
            return Ok(Cost::RemoveCounters {
                kind,
                count,
                subject,
            });
        }
        return Err(ts.err("expected `from <subj>` in remove-counter cost"));
    }

    // `Put <N> <kind> counter(s) on <subj>`
    if ts.eat_word("put") {
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else {
            ValueExpr::Const(1)
        };
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counter") || ts.eat_word("counters")) && ts.eat_word("on") {
            let subject = parse_subject_ref(ts)?;
            return Ok(Cost::PutCounters {
                kind,
                count,
                subject,
            });
        }
        return Err(ts.err("expected `counter(s) on <subj>` in put-counter cost"));
    }

    // `Mill <N> card(s)`
    if ts.eat_word("mill") {
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else {
            parse_value_expr(ts)?
        };
        if ts.eat_word("card") || ts.eat_word("cards") {
            return Ok(Cost::Mill(count));
        }
        return Err(ts.err("expected `card(s)` after `mill` in cost"));
    }

    // `Reveal <N> <filter> from your hand`
    if ts.eat_word("reveal") {
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else {
            ValueExpr::Const(1)
        };
        let filter = parse_object_filter(ts)?;
        let _ = ts.eat_phrase(&["from", "your", "hand"]);
        return Ok(Cost::RevealFromHand { count, filter });
    }

    Err(ts.err("expected cost atom"))
}
