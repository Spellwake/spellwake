//! Strictly-typed [`Cost`] and cost-list parser for `v2`.

use crate::v2::filter::{parse_counter_kind, parse_object_filter};
use crate::v2::ir::{Cost, ManaSymbol, ValueExpr};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, sep_by1};
use crate::v2::value::parse_value_expr;

/// Parse a comma-separated list of activation/additional costs (e.g. `{2}, {T}, Sacrifice ~`).
pub fn parse_cost_list(ts: &mut TokenStream<'_>) -> PResult<Vec<Cost>> {
    sep_by1(ts, parse_cost_atom, |s| {
        if s.eat_comma() {
            Ok(())
        } else {
            Err(s.err("expected `,` between costs"))
        }
    })
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

    // `Pay <val> life` / `Pay <mana>`
    if ts.eat_word("pay") {
        if let Some(mana) = ts.eat_mana_cost() {
            return Ok(Cost::Mana(mana));
        }
        let amt = parse_value_expr(ts)?;
        if ts.eat_word("life") {
            return Ok(Cost::PayLife(amt));
        }
        return Err(ts.err("expected `life` or mana after `pay`"));
    }

    // `Sacrifice ~` / `Sacrifice <N> <filter>`
    if ts.eat_word("sacrifice") {
        if ts.eat_tilde() {
            return Ok(Cost::SacrificeSelf);
        }
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else {
            ValueExpr::Const(1)
        };
        let filter = parse_object_filter(ts)?;
        return Ok(Cost::Sacrifice { count, filter });
    }

    // `Exile ~`
    if ts.eat_word("exile") && ts.eat_tilde() {
        return Ok(Cost::ExileSelf);
    }

    // `Discard <N> <filter>`
    if ts.eat_word("discard") {
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else {
            ValueExpr::Const(1)
        };
        if ts.eat_word("card") || ts.eat_word("cards") {
            return Ok(Cost::Discard {
                count,
                filter: None,
            });
        }
        let flt = parse_object_filter(ts)?;
        return Ok(Cost::Discard {
            count,
            filter: Some(flt),
        });
    }

    // `Remove <N> <kind> counter(s) from ~`
    if ts.eat_word("remove") {
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else {
            ValueExpr::Const(1)
        };
        let kind = parse_counter_kind(ts)?;
        if (ts.eat_word("counter") || ts.eat_word("counters"))
            && ts.eat_word("from")
            && ts.eat_tilde()
        {
            return Ok(Cost::RemoveCounters { kind, count });
        }
        return Err(ts.err("expected `counter(s) from ~` in remove-counter cost"));
    }

    Err(ts.err("expected cost atom"))
}
