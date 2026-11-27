//! Strictly-typed recursive [`ValueExpr`] parser for `v2`.

use crate::ast::{PlayerStatKind, Rounding, StatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref,
};
use crate::v2::ir::{
    AggregateOp, AnaphoraRef, ManaSymbol, PlayerRef, PriorActionKind, SubjectRef, TurnEventKind,
    ValueExpr, VarName,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};

/// Parse a strictly-typed dynamic numeric [`ValueExpr`].
pub fn parse_value_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    // `<N> times <expr>` (e.g. `three times the number of Angels you control`)
    {
        let cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false)
            && ts.eat_word("times")
        {
            let rhs = parse_value_expr(ts)?;
            return Ok(ValueExpr::Mul(Box::new(ValueExpr::Const(n)), Box::new(rhs)));
        }
        ts.restore(cp);
    }

    // `the amount of {E} paid this way`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "amount", "of"])
            && matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            )
        {
            ts.next_tok();
            if ts.eat_phrase(&["paid", "this", "way"]) {
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::EnergyPaidThisWay,
                ));
            }
        }
        ts.restore(cp);
    }

    // `the greatest/least <stat> among <filter>`
    {
        let cp = ts.checkpoint();
        let agg = if ts.eat_phrase(&["the", "greatest"]) || ts.eat_phrase(&["the", "highest"]) {
            Some(AggregateOp::Max)
        } else if ts.eat_phrase(&["the", "least"]) || ts.eat_phrase(&["the", "lowest"]) {
            Some(AggregateOp::Min)
        } else {
            None
        };
        if let Some(op) = agg {
            let stat = if ts.eat_phrase(&["mana", "value"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_word("power") {
                Some(StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if let Some(stat) = stat
                && (ts.eat_word("among") || ts.eat_word("of"))
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::AggregateStat {
                    op,
                    stat,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(cp);
    }

    // `the total power/toughness/mana value of <filter>`
    if ts.eat_phrase(&["the", "total", "power", "of"]) {
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(flt),
        });
    }
    if ts.eat_phrase(&["the", "total", "toughness", "of"]) {
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(flt),
        });
    }

    // `the number of ...`
    if ts.eat_phrase(&["the", "number", "of"]) {
        if ts.eat_phrase(&["opponents", "you", "have"]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["cards", "in", "your", "hand"]) {
            return Ok(ValueExpr::PlayerStat {
                player: PlayerRef::You,
                stat: PlayerStatKind::CardsInHand,
            });
        }
        let flt = parse_object_filter(ts)?;
        // Optional trailing `at that time` (e.g. delayed trigger X binding)
        let _ = ts.eat_phrase(&["at", "that", "time"]);
        return Ok(ValueExpr::Count(Box::new(flt)));
    }

    // `~'s power` / `~'s toughness` / `that creature's power` / `that creature's toughness`
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() {
            let _ = ts.eat_word("'s");
            if ts.eat_word("power") {
                return Ok(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::SelfObject),
                    stat: StatKind::Power,
                });
            }
            if ts.eat_word("toughness") {
                return Ok(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::SelfObject),
                    stat: StatKind::Toughness,
                });
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["that", "creature's", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["that", "creature's", "toughness"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Toughness,
        });
    }

    // `its mana value` / `its power` / `its toughness`
    if ts.eat_phrase(&["its", "mana", "value"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::ManaValue,
        });
    }
    if ts.eat_phrase(&["its", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["its", "toughness"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Toughness,
        });
    }

    // `that many` / `that much`
    if ts.eat_phrase(&["that", "many"]) || ts.eat_phrase(&["that", "much"]) {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }

    // `X`
    if ts.eat_word("x") {
        return Ok(ValueExpr::Var(VarName::X));
    }

    // Constant integer or number word
    if let Some(n) = ts.next_number(false) {
        return Ok(ValueExpr::Const(n));
    }

    Err(ts.err("expected numeric value expression"))
}

/// Parse the expression following `for each ` or `for every ` into a typed [`ValueExpr`].
pub fn parse_for_each_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    // `for every <N> <filter>` -> Div { expr: Count(flt), denom: N, rounding: Down }
    if ts.eat_phrase(&["for", "every"]) {
        let denom = ts
            .next_number(false)
            .ok_or_else(|| ts.err("expected divisor after `for every`"))?;
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::Div {
            expr: Box::new(ValueExpr::Count(Box::new(flt))),
            denom,
            rounding: Rounding::Down,
        });
    }

    if !ts.eat_phrase(&["for", "each"]) {
        return Err(ts.err("expected `for each` or `for every`"));
    }

    // `spell you've cast this turn [other than the first]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("spell") {
            let player = if ts.eat_word("you've") || ts.eat_word("you") {
                Some(PlayerRef::You)
            } else if ts.eat_phrase(&["an", "opponent"]) {
                Some(PlayerRef::AnyOpponent)
            } else {
                try_parse(ts, parse_player_ref).ok()
            };
            if ts.eat_phrase(&["cast", "this", "turn"]) {
                let exclude_first = ts.eat_phrase(&["other", "than", "the", "first"]);
                return Ok(ValueExpr::EventCountThisTurn {
                    event: TurnEventKind::SpellCast,
                    player,
                    exclude_first,
                });
            }
        }
        ts.restore(cp);
    }

    // `mana from <filter> that was spent to cast ~`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["mana", "from"]) {
            let flt = parse_object_filter(ts)?;
            if ts.eat_phrase(&["that", "was", "spent", "to", "cast"]) && ts.eat_tilde() {
                return Ok(ValueExpr::ManaSpentFromFilter(Box::new(flt)));
            }
        }
        ts.restore(cp);
    }

    // `card in your hand`
    if ts.eat_phrase(&["card", "in", "your", "hand"]) {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::You,
            stat: PlayerStatKind::CardsInHand,
        });
    }

    // `<kind> counter on <subj>` (e.g. `for each burden counter on ~`, `for each +1/+1 counter on it`)
    {
        let cp = ts.checkpoint();
        if let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
            && ts.eat_word("on")
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ValueExpr::CounterCount {
                subject: Box::new(subject),
                kind,
            });
        }
        ts.restore(cp);
    }

    // `creature exiled this way` -> PriorActionCount(ExiledThisWay) or Count(flt)
    let flt = parse_object_filter(ts)?;
    Ok(ValueExpr::Count(Box::new(flt)))
}
