//! Strictly-typed [`Cost`] and cost-list parser for `v2`.

use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{Cost, ManaSymbol, ObjectFilter, ValueExpr};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;

/// Parse a comma- or `and`-separated list of activation/additional costs (e.g. `{2}, {T}, Sacrifice ~`,
/// or `Sacrifice a white creature, a blue creature, and a black creature`,
/// or `reveal a Dinosaur card from your hand or pay {1}`).
pub fn parse_cost_list(ts: &mut TokenStream<'_>) -> PResult<Vec<Cost>> {
    let mut costs = vec![parse_cost_or_alt(ts)?];
    loop {
        let cp = ts.checkpoint();
        let has_sep = if ts.eat_comma() {
            let _ = ts.eat_word("and");
            true
        } else {
            ts.eat_word("and") || ts.eat_word("and/or")
        };
        if !has_sep {
            ts.restore(cp);
            break;
        }
        if let Ok(next) = try_parse(ts, parse_cost_or_alt) {
            costs.push(next);
            continue;
        }
        // Conjoined sacrifice/exile targets (`Sacrifice a Swamp and a Forest`, `Exile two cards from your graveyard and ~`)
        if matches!(
            costs.last(),
            Some(
                Cost::Sacrifice { .. } | Cost::SacrificeSelf | Cost::Exile { .. } | Cost::ExileSelf
            )
        ) {
            let is_exile = matches!(costs.last(), Some(Cost::Exile { .. } | Cost::ExileSelf));
            let scp = ts.checkpoint();
            if ts.eat_tilde() || ts.eat_word("it") {
                costs.push(if is_exile {
                    Cost::ExileSelf
                } else {
                    Cost::SacrificeSelf
                });
                continue;
            }
            let count = if ts.eat_word("all")
                || ts.eat_phrase(&["any", "number", "of"])
                || ts.eat_phrase(&["one", "or", "more"])
            {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else if let Some(n) = ts.next_number(true) {
                ValueExpr::Const(n)
            } else if ts.eat_word("x") {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else {
                ValueExpr::Const(1)
            };
            if let Ok(filter) = try_parse(ts, parse_object_filter) {
                costs.push(if is_exile {
                    Cost::Exile {
                        count,
                        filter,
                        from: Vec::new(),
                    }
                } else {
                    Cost::Sacrifice { count, filter }
                });
                continue;
            }
            ts.restore(scp);
        }
        ts.restore(cp);
        break;
    }
    Ok(costs)
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
        if let Some(scale) = crate::v2::parser::opt(ts, crate::v2::value::parse_for_each_expr) {
            return Ok(Cost::ScaledMana { base: mana, scale });
        }
        return Ok(Cost::Mana(mana));
    }

    // `Pay <val> life` / `Pay {E}...` / `Pay <mana> [for each <expr>]` (including gerund `paying ...` after `by`)
    if ts.eat_word("pay") || ts.eat_word("paying") {
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
            if let Some(scale) = crate::v2::parser::opt(ts, crate::v2::value::parse_for_each_expr) {
                return Ok(Cost::ScaledMana { base: mana, scale });
            }
            return Ok(Cost::Mana(mana));
        }
        if ts.eat_phrase(&["any", "amount", "of", "life"]) {
            return Ok(Cost::PayAnyAmountLife);
        }
        if ts.eat_phrase(&["any", "amount", "of", "mana"]) {
            return Ok(Cost::PayAnyAmountMana);
        }
        if ts.eat_phrase(&["life", "equal", "to"]) {
            let amt = parse_value_expr(ts)?;
            return Ok(Cost::PayLife(amt));
        }
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("half"))
            && let Ok(amt) = try_parse(ts, parse_value_expr)
        {
            let _ = ts.eat_word("life");
            return Ok(Cost::PayLife(amt));
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

    // `Behold a/an <flt>` (`Osseous Exhale`, Tarkir Dragonstorm)
    if ts.eat_word("behold") {
        let filter = parse_object_filter(ts)?;
        return Ok(Cost::Behold(filter));
    }

    // `Sacrifice ~` / `Sacrifice it` / `Sacrifice [all | any number of | one or more | N] <filter> [or <filter2>]`
    if ts.eat_word("sacrifice") || ts.eat_word("sacrificing") {
        if ts.eat_tilde() || ts.eat_word("it") {
            return Ok(Cost::SacrificeSelf);
        }
        let count = if ts.eat_word("all")
            || ts.eat_phrase(&["any", "number", "of"])
            || ts.eat_phrase(&["one", "or", "more"])
        {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let mut filter = parse_object_filter(ts)?;
        // Support `Sacrifice a Food or another creature` (`Malevolent Noble`)
        let ocp = ts.checkpoint();
        if ts.eat_word("or") {
            let _ = ts.eat_word("a") || ts.eat_word("an");
            if let Ok(other) = try_parse(ts, parse_object_filter) {
                filter = ObjectFilter {
                    predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![filter, other])],
                    ..Default::default()
                };
            } else {
                ts.restore(ocp);
            }
        }
        return Ok(Cost::Sacrifice { count, filter });
    }

    // `Forage`
    if ts.eat_word("forage") {
        return Ok(Cost::Forage);
    }

    // `Unattach ~`
    if ts.eat_word("unattach") && ts.eat_tilde() {
        return Ok(Cost::UnattachSelf);
    }

    // `Blight <val>`
    if ts.eat_word("blight") {
        let amt = parse_value_expr(ts)?;
        return Ok(Cost::Blight(amt));
    }

    // `Waterbend {M}`
    if ts.eat_word("waterbend")
        && let Some(mc) = ts.eat_mana_cost()
    {
        return Ok(Cost::Waterbend(mc));
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

    // `Exile ~` / `Exile the top [N] card(s) of <zone>` / `Exile <N> <filter> [from <zone>]` (including gerund `exiling ...` after `by`)
    if ts.eat_word("exile") || ts.eat_word("exiling") {
        if ts.eat_tilde() || ts.eat_word("it") {
            let _ = ts.eat_word("from") && try_parse(ts, parse_zone_ref).is_ok();
            return Ok(Cost::ExileSelf);
        }
        if ts.eat_phrase(&["the", "top"]) {
            let t_cp = ts.checkpoint();
            let count = if ts.eat_word("card") {
                Some(ValueExpr::Const(1))
            } else if let Ok(n) = try_parse(ts, parse_value_expr)
                && (ts.eat_word("cards") || ts.eat_word("card"))
            {
                Some(n)
            } else {
                None
            };
            if let Some(count) = count
                && ts.eat_word("of")
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(Cost::ExileTop { zone, count });
            }
            ts.restore(t_cp);
            if let Ok(filter) = try_parse(ts, parse_object_filter)
                && ts.eat_word("of")
                && let Ok(zone) = try_parse(ts, parse_zone_ref)
            {
                return Ok(Cost::Exile {
                    count: ValueExpr::Const(1),
                    filter,
                    from: vec![zone],
                });
            }
            ts.restore(t_cp);
        }
        let mut count =
            if ts.eat_phrase(&["any", "number", "of"]) || ts.eat_phrase(&["one", "or", "more"]) {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else if let Some(n) = ts.next_number(true) {
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
        if ts.eat_phrase(&["equal", "to"]) {
            count = parse_value_expr(ts)?;
        }
        return Ok(Cost::Exile {
            count,
            filter,
            from,
        });
    }

    // `Untap <N> [tapped] <filter>` (`Crackleburr`, etc.)
    if ts.eat_word("untap") {
        if ts.eat_tilde() {
            return Ok(Cost::UntapSelf);
        }
        let count =
            if ts.eat_phrase(&["any", "number", "of"]) || ts.eat_phrase(&["one", "or", "more"]) {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else if let Some(n) = ts.next_number(true) {
                ValueExpr::Const(n)
            } else if ts.eat_word("x") {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else {
                ValueExpr::Const(1)
            };
        let filter = parse_object_filter(ts)?;
        return Ok(Cost::UntapTapped { count, filter });
    }

    // `Tap ~` / `Tap <N> untapped <filter>`
    if ts.eat_word("tap") {
        if ts.eat_tilde() {
            return Ok(Cost::TapSelf);
        }
        let count =
            if ts.eat_phrase(&["any", "number", "of"]) || ts.eat_phrase(&["one", "or", "more"]) {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else if let Some(n) = ts.next_number(true) {
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
            || ts.eat_phrase(&["to", "their", "owner's", "hand"])
            || ts.eat_phrase(&["to", "your", "hand"])
        {
            return Ok(Cost::ReturnToHand { subject });
        }
        return Err(ts.err("expected `to its owner's hand` in return cost"));
    }

    // `Discard your hand` / `Discard ~` / `Discard <N> <filter> [at random]`
    if ts.eat_word("discard") || ts.eat_word("discarding") {
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
        let count =
            if ts.eat_phrase(&["any", "number", "of"]) || ts.eat_phrase(&["one", "or", "more"]) {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else if let Some(n) = ts.next_number(true) {
                ValueExpr::Const(n)
            } else if ts.eat_word("x") {
                ValueExpr::Var(crate::v2::ir::VarName::X)
            } else {
                ValueExpr::Const(1)
            };
        let dcp = ts.checkpoint();
        let filter = if (ts.eat_word("card") || ts.eat_word("cards"))
            && !ts.peek_word().is_some_and(|w| {
                let l = w.to_ascii_lowercase();
                ["with", "that", "from", "you", "named", "in", "whose"].contains(&l.as_str())
            }) {
            None
        } else {
            ts.restore(dcp);
            Some(parse_object_filter(ts)?)
        };
        let random = ts.eat_phrase(&["at", "random"]);
        return Ok(Cost::Discard {
            count,
            filter,
            random,
        });
    }

    // `Remove <N | all | any number of | one or more> [<kind>] counter(s) from [among] <subj>`
    if ts.eat_word("remove") || ts.eat_word("removing") {
        let count = if ts.eat_word("all")
            || ts.eat_phrase(&["any", "number", "of"])
            || ts.eat_phrase(&["one", "or", "more"])
        {
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
        // Optional `or a/an <kind2> counter` (`Ion Storm`)
        let mut kind2_opt = None;
        {
            let ocp = ts.checkpoint();
            if ts.eat_word("or") {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                if let Ok(ck2) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                {
                    kind2_opt = Some(ck2);
                } else {
                    ts.restore(ocp);
                }
            }
        }
        if ts.eat_word("from") {
            let _ = ts.eat_word("among");
            let subject = if ts.eat_tilde() {
                None
            } else {
                Some(parse_subject_ref(ts)?)
            };
            if let Some(kind2) = kind2_opt {
                return Ok(Cost::RemoveEitherCounter {
                    kind1: kind,
                    kind2,
                    subject,
                });
            }
            return Ok(Cost::RemoveCounters {
                kind,
                count,
                subject,
            });
        }
        return Err(ts.err("expected `from <subj>` in remove-counter cost"));
    }

    // `Put <N> <kind> counter(s) on <subj>` OR `Put <subj> <dest>`
    if ts.eat_word("put") {
        let pcp = ts.checkpoint();
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        if let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(Cost::PutCounters {
                kind,
                count,
                subject,
            });
        }
        ts.restore(pcp);
        if let Ok(subject) = try_parse(ts, parse_subject_ref)
            && let Ok((dest, _)) = try_parse(ts, crate::v2::filter::parse_zone_dest)
        {
            return Ok(Cost::PutZone { subject, dest });
        }
        return Err(ts.err("expected `counter(s) on <subj>` or zone move in put cost"));
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

    // `Reveal ~ from your hand` / `Reveal <N> <filter> from your hand`
    if ts.eat_word("reveal") {
        if ts.eat_tilde() {
            let _ = ts.eat_phrase(&["from", "your", "hand"]);
            return Ok(Cost::RevealFromHand {
                count: ValueExpr::Const(1),
                filter: ObjectFilter {
                    any_card: true,
                    ..Default::default()
                },
            });
        }
        let count = if let Some(n) = ts.next_number(true) {
            ValueExpr::Const(n)
        } else if ts.eat_word("x") {
            ValueExpr::Var(crate::v2::ir::VarName::X)
        } else {
            ValueExpr::Const(1)
        };
        let filter = parse_object_filter(ts)?;
        let _ = ts.eat_phrase(&["from", "your", "hand"]);
        return Ok(Cost::RevealFromHand { count, filter });
    }

    // `Choose <subj1> [or <subj2>]` (e.g. `As an additional cost to cast ~, choose a creature you control or a warped creature card you own in exile`)
    if ts.eat_word("choose") {
        let first = parse_subject_ref(ts)?;
        let cp = ts.checkpoint();
        if ts.eat_word("or")
            && let Ok(second) = try_parse(ts, parse_subject_ref)
        {
            return Ok(Cost::ChooseObject(crate::v2::ir::SubjectRef::Compound(
                vec![first, second],
            )));
        }
        ts.restore(cp);
        return Ok(Cost::ChooseObject(first));
    }

    Err(ts.err("expected cost atom"))
}
