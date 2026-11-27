//! Strictly-typed recursive [`ValueExpr`] parser for `v2`.

use crate::ast::{PlayerStatKind, Rounding, StatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AggregateOp, AnaphoraRef, ManaSymbol, PlayerRef, PriorActionKind, SubjectRef, TurnEventKind,
    ValueExpr, VarName,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};

/// Parse a strictly-typed dynamic numeric [`ValueExpr`] (supports infix `plus` / `minus`).
pub fn parse_value_expr(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    let mut lhs = parse_value_atom(ts)?;
    loop {
        if ts.eat_word("plus") {
            let rhs = parse_value_atom(ts)?;
            lhs = ValueExpr::Add(Box::new(lhs), Box::new(rhs));
            continue;
        }
        if ts.eat_word("minus") {
            let rhs = parse_value_atom(ts)?;
            lhs = ValueExpr::Sub(Box::new(lhs), Box::new(rhs));
            continue;
        }
        break;
    }
    Ok(lhs)
}

fn parse_value_atom(ts: &mut TokenStream<'_>) -> PResult<ValueExpr> {
    // `twice <expr>` / `<N> times <expr>` / `half <expr> [, rounded up/down]`
    if ts.eat_word("twice") {
        let rhs = parse_value_expr(ts)?;
        return Ok(ValueExpr::Mul(Box::new(ValueExpr::Const(2)), Box::new(rhs)));
    }
    if ts.eat_word("half") {
        let expr = parse_value_expr(ts)?;
        let _ = ts.eat_comma();
        let rounding = if ts.eat_phrase(&["rounded", "up"]) {
            Rounding::Up
        } else {
            let _ = ts.eat_phrase(&["rounded", "down"]);
            Rounding::Down
        };
        return Ok(ValueExpr::Div {
            expr: Box::new(expr),
            denom: 2,
            rounding,
        });
    }
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
    // `the [amount of] excess damage dealt [to <subj>] this way` / `that excess damage`
    {
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["that", "excess", "damage"]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::ExcessDamageThisWay,
            ));
        }
        if ts.eat_phrase(&["the", "amount", "of", "excess", "damage", "dealt"])
            || ts.eat_phrase(&["the", "excess", "damage", "dealt"])
        {
            if ts.eat_word("to") {
                let _ = try_parse(ts, parse_subject_ref);
            }
            let _ = ts.eat_phrase(&["this", "way"]);
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::ExcessDamageThisWay,
            ));
        }
        ts.restore(ecp);
    }
    // `the [total] mana value/power/toughness of <subj | flt>` (`the mana value of the returned creature`, `the toughness of the card returned this way`, `the total power of the sacrificed creatures`)
    {
        let ocp = ts.checkpoint();
        if ts.eat_word("the") {
            let is_total = ts.eat_word("total");
            let stat = if ts.eat_phrase(&["mana", "value", "of"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_phrase(&["power", "of"]) {
                Some(StatKind::Power)
            } else if ts.eat_phrase(&["toughness", "of"]) {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if let Some(stat) = stat {
                if !is_total {
                    // Pick the longer parse between `parse_subject_ref` and `the + parse_object_filter`
                    // so `the toughness of the card returned this way` doesn't stop after `the card`!
                    let best_cp = ts.checkpoint();
                    let subj_res = try_parse(ts, parse_subject_ref).ok();
                    let subj_pos = ts.pos;
                    ts.restore(best_cp);
                    let flt_res = if ts.eat_word("the") || ts.eat_word("all") {
                        try_parse(ts, parse_object_filter).ok()
                    } else {
                        None
                    };
                    let flt_pos = ts.pos;
                    if let Some(flt) = flt_res
                        && flt_pos > subj_pos
                    {
                        return Ok(ValueExpr::ObjectStat {
                            subject: Box::new(SubjectRef::Group(Box::new(flt))),
                            stat,
                        });
                    }
                    if let Some(subject) = subj_res {
                        ts.pos = subj_pos;
                        return Ok(ValueExpr::ObjectStat {
                            subject: Box::new(subject),
                            stat,
                        });
                    }
                    ts.restore(best_cp);
                }
                let _ = ts.eat_word("all") || ts.eat_word("the") || ts.eat_word("those");
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    return Ok(ValueExpr::AggregateStat {
                        op: AggregateOp::Sum,
                        stat,
                        filter: Box::new(flt),
                    });
                }
            }
        }
        ts.restore(ocp);
    }
    if ts.eat_phrase(&["the", "damage", "prevented", "this", "way"])
        || ts.eat_phrase(&["the", "amount", "of", "damage", "prevented", "this", "way"])
    {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::DamagePreventedThisWay,
        ));
    }
    if ts.eat_phrase(&["the", "life", "paid", "as"]) && ts.eat_tilde() && ts.eat_word("entered") {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::LifePaidAsEntered,
        ));
    }
    if ts.eat_phrase(&["their", "total", "power"]) {
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(crate::v2::ir::ObjectFilter {
                any_permanent: true,
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&["their", "total", "toughness"]) {
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(crate::v2::ir::ObjectFilter {
                any_permanent: true,
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&["the", "life", "lost", "this", "way"])
        || ts.eat_phrase(&["the", "life", "gained", "this", "way"])
        || ts.eat_phrase(&["the", "damage", "dealt", "this", "way"])
        || ts.eat_phrase(&["the", "damage", "dealt"])
    {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::DealtDamageThisWay,
        ));
    }

    // `your speed` / `that player's speed` / `the total mana value of <flt>` / `the amount of {S} spent to cast ~` / `the total amount of mana paid this way`
    if ts.eat_phrase(&["the", "result"])
        || (!ts.peek_phrase(&["the", "difference", "between"])
            && ts.eat_phrase(&["the", "difference"]))
    {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    if ts.eat_phrase(&[
        "the",
        "total",
        "amount",
        "of",
        "life",
        "your",
        "opponents",
        "have",
        "lost",
        "this",
        "turn",
    ]) || ts.eat_phrase(&[
        "the",
        "total",
        "amount",
        "of",
        "life",
        "your",
        "opponents",
        "lost",
        "this",
        "turn",
    ]) {
        return Ok(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::LifeLost,
            player: Some(PlayerRef::EachOpponent),
            exclude_first: false,
        });
    }
    if ts.eat_phrase(&["your", "speed"]) {
        return Ok(ValueExpr::Speed(PlayerRef::You));
    }
    if ts.eat_phrase(&["that", "player's", "speed"])
        || ts.eat_phrase(&["that", "opponent's", "speed"])
        || ts.eat_phrase(&["their", "speed"])
    {
        return Ok(ValueExpr::Speed(PlayerRef::ThatPlayer));
    }
    if ts.eat_phrase(&[
        "the",
        "total",
        "amount",
        "of",
        "noncombat",
        "damage",
        "dealt",
        "to",
        "your",
        "opponents",
        "this",
        "turn",
    ]) || ts.eat_phrase(&[
        "the",
        "amount",
        "of",
        "noncombat",
        "damage",
        "dealt",
        "to",
        "your",
        "opponents",
        "this",
        "turn",
    ]) {
        return Ok(ValueExpr::DamageDealtToOpponentsThisTurn {
            noncombat_only: true,
        });
    }
    if ts.eat_phrase(&[
        "the",
        "total",
        "amount",
        "of",
        "damage",
        "dealt",
        "to",
        "your",
        "opponents",
        "this",
        "turn",
    ]) || ts.eat_phrase(&[
        "the",
        "amount",
        "of",
        "damage",
        "dealt",
        "to",
        "your",
        "opponents",
        "this",
        "turn",
    ]) {
        return Ok(ValueExpr::DamageDealtToOpponentsThisTurn {
            noncombat_only: false,
        });
    }
    if ts.eat_phrase(&[
        "the", "total", "amount", "of", "mana", "paid", "this", "way",
    ]) || ts.eat_phrase(&["the", "amount", "of", "mana", "paid", "this", "way"])
    {
        return Ok(ValueExpr::ManaPaidThisWay);
    }
    if ts.eat_phrase(&["the", "total", "mana", "value", "of"]) {
        let _ = ts.eat_word("all") || ts.eat_word("the") || ts.eat_word("those");
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(flt),
        });
    }

    // `half your/their/that player's life [total], rounded up/down` (`Lurking Evil`, `Phyrexian Processor`)
    {
        let hcp = ts.checkpoint();
        if ts.eat_word("half") {
            let player =
                if ts.eat_phrase(&["your", "life", "total"]) || ts.eat_phrase(&["your", "life"]) {
                    Some(PlayerRef::You)
                } else if ts.eat_phrase(&["their", "life", "total"])
                    || ts.eat_phrase(&["their", "life"])
                    || ts.eat_phrase(&["that", "player's", "life", "total"])
                    || ts.eat_phrase(&["that", "player's", "life"])
                {
                    Some(PlayerRef::ThatPlayer)
                } else if ts.eat_phrase(&["target", "player's", "life", "total"])
                    || ts.eat_phrase(&["target", "player's", "life"])
                {
                    Some(PlayerRef::TargetPlayer)
                } else if ts.eat_phrase(&["target", "opponent's", "life", "total"])
                    || ts.eat_phrase(&["target", "opponent's", "life"])
                {
                    Some(PlayerRef::TargetOpponent)
                } else {
                    None
                };
            if let Some(p) = player {
                let _ = ts.eat_comma();
                let rounding = if ts.eat_phrase(&["rounded", "up"]) {
                    Rounding::Up
                } else {
                    let _ = ts.eat_phrase(&["rounded", "down"]);
                    Rounding::Down
                };
                return Ok(ValueExpr::Div {
                    expr: Box::new(ValueExpr::PlayerStat {
                        player: p,
                        stat: PlayerStatKind::LifeTotal,
                    }),
                    denom: 2,
                    rounding,
                });
            }
        }
        ts.restore(hcp);
    }

    // `your life total` / `that player's life total` / `your devotion to <colors>`
    if ts.eat_phrase(&["your", "life", "total"]) {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::You,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if ts.eat_phrase(&["that", "player's", "life", "total"])
        || ts.eat_phrase(&["their", "life", "total"])
    {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::ThatPlayer,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if ts.eat_phrase(&["your", "devotion", "to"]) {
        let mut mask = crate::ast::ColorMask::EMPTY;
        loop {
            if let Some(w) = ts.peek_word()
                && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                mask = mask.union(c);
                let _ = ts.eat_comma();
                let _ = ts.eat_word("and");
                continue;
            }
            break;
        }
        if !mask.is_empty() {
            return Ok(ValueExpr::Devotion(mask));
        }
    }

    // `the amount of {E} paid this way` / `the amount of life you gained [this turn]` / `the amount of damage dealt`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "amount", "of"]) {
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Snow))
            ) {
                ts.next_tok();
                if ts.eat_phrase(&["spent", "to", "cast"]) && (ts.eat_tilde() || ts.eat_word("it"))
                {
                    return Ok(ValueExpr::SnowManaSpent);
                }
            }
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(ManaSymbol::Energy))
            ) {
                ts.next_tok();
                if ts.eat_phrase(&["paid", "this", "way"]) {
                    return Ok(ValueExpr::PriorActionCount(
                        PriorActionKind::EnergyPaidThisWay,
                    ));
                }
            }
            if ts.eat_phrase(&["life", "you", "gained"])
                || ts.eat_phrase(&["life", "you've", "gained"])
                || ts.eat_phrase(&["life", "they", "gained"])
                || ts.eat_phrase(&["life", "they've", "gained"])
                || ts.eat_phrase(&["life", "that", "player", "gained"])
                || ts.eat_phrase(&["life", "gained"])
            {
                let _ = ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["this", "way"]);
                return Ok(ValueExpr::EventCountThisTurn {
                    event: TurnEventKind::LifeGained,
                    player: Some(PlayerRef::You),
                    exclude_first: false,
                });
            }
            if ts.eat_phrase(&["life", "you", "lost"])
                || ts.eat_phrase(&["life", "you've", "lost"])
                || ts.eat_phrase(&["life", "they", "lost"])
                || ts.eat_phrase(&["life", "they've", "lost"])
                || ts.eat_phrase(&["life", "that", "player", "lost"])
                || ts.eat_phrase(&["life", "lost"])
                || ts.eat_phrase(&["life", "your", "opponents", "lost"])
            {
                let _ = ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["this", "way"]);
                return Ok(ValueExpr::EventCountThisTurn {
                    event: TurnEventKind::LifeLost,
                    player: Some(PlayerRef::You),
                    exclude_first: false,
                });
            }
            if ts.eat_phrase(&["mana", "spent", "to", "cast"])
                && try_parse(ts, parse_subject_ref).is_ok()
            {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
            if ts.eat_phrase(&["damage", "dealt"]) {
                let _ = ts.eat_phrase(&["this", "way"]) || ts.eat_phrase(&["this", "turn"]);
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
                ));
            }
            if ts.eat_word("damage")
                && try_parse(ts, parse_subject_ref).is_ok()
                && ts.eat_word("dealt")
            {
                if ts.eat_word("to") {
                    let _ = try_parse(ts, parse_subject_ref);
                }
                let _ = ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["this", "way"]);
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
                ));
            }
        }
        ts.restore(cp);
    }

    // `the greatest/highest/least/lowest <stat> among <filter> [and <filter2>] [as you cast ~]`
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
            if ts.eat_phrase(&["life", "total", "among", "all", "players"])
                || ts.eat_phrase(&["life", "total", "among", "players"])
            {
                return Ok(if op == AggregateOp::Max {
                    ValueExpr::HighestLifeTotal
                } else {
                    ValueExpr::PlayerStat {
                        player: PlayerRef::LowestLifePlayer,
                        stat: PlayerStatKind::LifeTotal,
                    }
                });
            }
            let stat = if ts.eat_phrase(&["mana", "value"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_phrase(&["power", "and/or", "toughness"])
                || ts.eat_phrase(&["power", "or", "toughness"])
                || ts.eat_word("power")
            {
                Some(StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if stat.is_none()
                && ts.eat_phrase(&["number", "of"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::Count(Box::new(flt)));
            }
            if let Some(stat) = stat
                && (ts.eat_word("among") || ts.eat_word("of"))
                && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            {
                let acp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Ok(other) = try_parse(ts, parse_object_filter)
                {
                    flt = crate::v2::ir::ObjectFilter {
                        predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![flt, other])],
                        ..Default::default()
                    };
                } else {
                    ts.restore(acp);
                }
                let _ = (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde())
                    || ts.eat_phrase(&["as", "you", "activate", "this", "ability"]);
                return Ok(ValueExpr::AggregateStat {
                    op,
                    stat,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(cp);
    }

    // `the total power/toughness/mana value of [the sacrificed creatures | all <filter> | <filter>]`
    if ts.eat_phrase(&[
        "the",
        "total",
        "power",
        "of",
        "the",
        "sacrificed",
        "creatures",
    ]) || ts.eat_phrase(&[
        "the",
        "total",
        "power",
        "of",
        "all",
        "sacrificed",
        "creatures",
    ]) {
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(crate::v2::ir::ObjectFilter {
                any_types: crate::ast::CardTypeMask::CREATURE,
                predicates: vec![crate::v2::ir::FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::SacrificedThisWay,
                )],
                ..Default::default()
            }),
        });
    }
    if ts.eat_phrase(&["the", "total", "power", "of"]) {
        let _ = ts.eat_word("all") || ts.eat_word("the");
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(flt),
        });
    }
    if ts.eat_phrase(&["the", "highest", "life", "total", "among", "all", "players"])
        || ts.eat_phrase(&["the", "highest", "life", "total", "among", "players"])
    {
        return Ok(ValueExpr::HighestLifeTotal);
    }
    if ts.eat_phrase(&["the", "amassed", "army's", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["the", "total", "mana", "value", "of"]) {
        let _ = ts.eat_word("all") || ts.eat_word("the") || ts.eat_word("those");
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(flt),
        });
    }
    if ts.eat_phrase(&["the", "total", "toughness", "of"]) {
        let _ = ts.eat_word("all") || ts.eat_word("the");
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(flt),
        });
    }
    // `the greatest/highest power/toughness/mana value among <flt> [and <flt2>] [as you cast ~]`
    {
        let gcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "greatest"]) || ts.eat_phrase(&["the", "highest"]) {
            let stat = if ts.eat_phrase(&["mana", "value"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_phrase(&["power", "and/or", "toughness"])
                || ts.eat_phrase(&["power", "or", "toughness"])
                || ts.eat_word("power")
            {
                Some(StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if stat.is_none()
                && ts.eat_phrase(&["number", "of"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::Count(Box::new(flt)));
            }
            if let Some(stat) = stat
                && ts.eat_word("among")
                && let Ok(mut flt) = try_parse(ts, parse_object_filter)
            {
                let acp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Ok(other) = try_parse(ts, parse_object_filter)
                {
                    flt = crate::v2::ir::ObjectFilter {
                        predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![flt, other])],
                        ..Default::default()
                    };
                } else {
                    ts.restore(acp);
                }
                let _ = ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde();
                return Ok(ValueExpr::AggregateStat {
                    op: AggregateOp::Max,
                    stat,
                    filter: Box::new(flt),
                });
            }
        }
        ts.restore(gcp);
    }
    // `the life <player> lost/gained this turn` (`Warlock Class`, `Archfiend of Despair`, `Wound Reflection`, `Children of Korlis`)
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "life"]) || ts.eat_phrase(&["the", "amount", "of", "life"]) {
            let p_opt = if ts.eat_word("you've") {
                Some(PlayerRef::You)
            } else if ts.eat_word("they've") {
                Some(PlayerRef::ThatPlayer)
            } else {
                try_parse(ts, parse_player_ref).ok()
            };
            if let Some(player) = p_opt {
                if ts.eat_phrase(&["lost", "this", "turn"]) {
                    return Ok(ValueExpr::EventCountThisTurn {
                        event: TurnEventKind::LifeLost,
                        player: Some(player),
                        exclude_first: false,
                    });
                }
                if ts.eat_phrase(&["gained", "this", "turn"]) {
                    return Ok(ValueExpr::EventCountThisTurn {
                        event: TurnEventKind::LifeGained,
                        player: Some(player),
                        exclude_first: false,
                    });
                }
            }
        }
        ts.restore(lcp);
    }
    // `the damage [already] dealt to <player> this turn` (`Final Punishment`)
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "damage"]) {
            let _ = ts.eat_word("already");
            if ts.eat_phrase(&["dealt", "to"])
                && let Ok(player) = try_parse(ts, parse_player_ref)
                && ts.eat_phrase(&["this", "turn"])
            {
                return Ok(ValueExpr::DamageDealtToPlayerThisTurn(player));
            }
        }
        ts.restore(dcp);
    }
    // `the difference between <subj>'s power and [its] toughness` (`Jaws of Defeat`)
    {
        let diff_cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "difference", "between"]) {
            let subj_opt = if ts.eat_phrase(&["that", "creature's"]) {
                Some(SubjectRef::Anaphora(AnaphoraRef::ThatCreature))
            } else if ts.eat_phrase(&["target", "creature's"]) {
                Some(SubjectRef::TargetObject {
                    filter: Box::new(crate::v2::ir::ObjectFilter {
                        any_types: crate::ast::CardTypeMask::CREATURE,
                        ..Default::default()
                    }),
                    count: crate::v2::ir::TargetCount::Exact(1),
                    another: false,
                })
            } else if ts.eat_word("its") {
                Some(SubjectRef::Anaphora(AnaphoraRef::It))
            } else if let Ok(s) = try_parse(ts, parse_subject_ref)
                && ts.eat_word("'s")
            {
                Some(s)
            } else {
                None
            };
            if let Some(subj) = subj_opt
                && ts.eat_word("power")
                && ts.eat_word("and")
            {
                let _ = ts.eat_word("its");
                if ts.eat_word("toughness") {
                    return Ok(ValueExpr::StatDifference(Box::new(subj)));
                }
            }
        }
        ts.restore(diff_cp);
    }

    if ts.eat_phrase(&["that", "amount"]) || ts.eat_phrase(&["that", "damage"]) {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::DealtDamageThisWay,
        ));
    }
    if ts.eat_phrase(&["the", "spell's", "mana", "value"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatSpell)),
            stat: StatKind::ManaValue,
        });
    }
    if ts.eat_phrase(&["the", "milled", "card's", "mana", "value"])
        || ts.eat_phrase(&["the", "discarded", "card's", "mana", "value"])
        || ts.eat_phrase(&["the", "exiled", "card's", "mana", "value"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(crate::v2::ir::AnaphoraRef::ThatCard)),
            stat: StatKind::ManaValue,
        });
    }
    if ts.eat_phrase(&["the", "power", "of", "the", "creature", "that", "died"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::ThatCreature,
            )),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["the", "toughness", "of", "the", "creature", "that", "died"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::ThatCreature,
            )),
            stat: StatKind::Toughness,
        });
    }
    if ts.eat_phrase(&["the", "creature's", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::ThatCreature,
            )),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["the", "creature's", "toughness"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(
                crate::v2::ir::AnaphoraRef::ThatCreature,
            )),
            stat: StatKind::Toughness,
        });
    }
    // `[the total | the] number of ...` (`Crackling Drake`, `Beacon Bolt`, `Multani, Maro-Sorcerer`, etc.)
    if ts.eat_phrase(&["the", "total", "number", "of"]) || ts.eat_phrase(&["the", "number", "of"]) {
        if ts.eat_phrase(&["permanents", "you've", "sacrificed", "this", "turn"])
            || ts.eat_phrase(&["permanents", "you", "sacrificed", "this", "turn"])
        {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::SacrificedThisWay,
            ));
        }
        if ts.eat_phrase(&["tokens", "you", "created", "this", "turn"])
            || ts.eat_phrase(&["tokens", "you've", "created", "this", "turn"])
        {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::CreatedThisWay));
        }
        if ts.eat_phrase(&[
            "times", "this", "ability", "has", "resolved", "this", "turn",
        ]) {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        if ts.eat_phrase(&[
            "spells", "you've", "cast", "from", "anywhere", "other", "than", "your", "hand",
            "this", "turn",
        ]) || ts.eat_phrase(&[
            "spells", "you", "cast", "from", "anywhere", "other", "than", "your", "hand", "this",
            "turn",
        ]) {
            return Ok(ValueExpr::EventCountThisTurn {
                event: TurnEventKind::SpellCast,
                player: Some(PlayerRef::You),
                exclude_first: false,
            });
        }
        {
            let ocp = ts.checkpoint();
            let _ = ts.eat_word("your");
            if ts.eat_phrase(&["opponents", "who"])
                || ts.eat_phrase(&["players", "who"])
                || ts.eat_phrase(&["opponents", "that"])
                || ts.eat_phrase(&["players", "that"])
            {
                if ts.eat_word("control") {
                    if ts.eat_phrase(&["at", "least"]) {
                        let _ = ts.next_number(false);
                    }
                    let _ = ts.eat_word("more");
                    if try_parse(ts, parse_object_filter).is_ok() {
                        let _ = ts.eat_phrase(&["than", "you"]);
                        return Ok(ValueExpr::OpponentsCount);
                    }
                }
                if ts.eat_phrase(&["investigated", "this", "way"])
                    || ts.eat_phrase(&["did", "this", "way"])
                {
                    return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
                }
                if ts.eat_phrase(&["were", "dealt"]) && {
                    let _ = ts.eat_word("combat");
                    ts.eat_word("damage")
                } {
                    if ts.eat_word("by") {
                        let _ = try_parse(ts, parse_subject_ref);
                        if ts.eat_word("or") {
                            let _ = try_parse(ts, parse_subject_ref);
                        }
                    }
                    let _ = ts.eat_phrase(&["this", "turn"]);
                    return Ok(ValueExpr::PlayersWhoDidThisTurn {
                        event: TurnEventKind::LifeLost,
                        opponents_only: true,
                    });
                }
            }
            ts.restore(ocp);
        }
        if ts.eat_phrase(&["players", "being", "attacked"])
            || ts.eat_phrase(&["players", "you", "attacked", "this", "combat"])
            || ts.eat_phrase(&["players", "you", "attacked", "this", "turn"])
        {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["different", "mana", "values", "among", "cards", "in"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ValueExpr::DistinctManaValues(zref));
        }
        if ts.eat_phrase(&["cards", "in", "all", "players'", "hands"]) {
            return Ok(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                any_card: true,
                zones: vec![crate::v2::ir::ZoneRef {
                    zone: crate::v2::ir::ZoneKind::Hand,
                    owner: crate::v2::ir::ZoneOwner::All,
                }],
                ..Default::default()
            })));
        }
        {
            let gy_cp = ts.checkpoint();
            if ts.eat_word("cards")
                && {
                    let _ = ts.eat_phrase(&["that", "were"]);
                    ts.eat_phrase(&["put", "into"])
                }
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_word("from")
            {
                let _ = ts.eat_word("anywhere") || try_parse(ts, parse_zone_ref).is_ok();
                if ts.eat_word("or") {
                    let _ = ts.eat_word("library")
                        || ts.eat_word("hand")
                        || ts.eat_word("graveyard")
                        || try_parse(ts, parse_zone_ref).is_ok();
                }
                let _ = ts.eat_phrase(&["this", "turn"]);
                return Ok(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                    any_card: true,
                    zones: vec![zref],
                    ..Default::default()
                })));
            }
            ts.restore(gy_cp);
        }
        // Chroma: `the number of <color> mana symbols in the mana costs of <flt>` (`Heartlash Cinder`)
        {
            let ch_cp = ts.checkpoint();
            if let Some(w) = ts.peek_word()
                && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                if ts.eat_phrase(&["mana", "symbols", "in", "the", "mana", "costs", "of"])
                    && try_parse(ts, parse_object_filter).is_ok()
                {
                    return Ok(ValueExpr::Devotion(c));
                }
            }
            ts.restore(ch_cp);
        }
        if ts.eat_phrase(&["colors", "among"]) {
            if let Ok(flt) = try_parse(ts, parse_object_filter) {
                return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Group(
                    Box::new(flt),
                ))));
            }
            if let Ok(subj) = try_parse(ts, parse_subject_ref) {
                return Ok(ValueExpr::ColorCount(Box::new(subj)));
            }
        }
        {
            let c2cp = ts.checkpoint();
            if ts.eat_word("colors")
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && (ts.eat_word("is") || ts.eat_word("are"))
            {
                return Ok(ValueExpr::ColorCount(Box::new(subj)));
            }
            ts.restore(c2cp);
        }
        {
            let kcp = ts.checkpoint();
            if ts.eat_word("times")
                && (ts.eat_tilde() || ts.eat_word("it"))
                && ts.eat_phrase(&["was", "kicked"])
            {
                return Ok(ValueExpr::TimesKicked);
            }
            ts.restore(kcp);
            if ts.eat_word("times")
                && (ts.eat_tilde() || ts.eat_word("it"))
                && ts.eat_phrase(&["has", "mutated"])
            {
                return Ok(ValueExpr::MutatedCount);
            }
            ts.restore(kcp);
        }
        // `the number of different kinds of counters among <flt>` (`Perrie, the Pulverizer`)
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["different", "kinds", "of", "counters", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::DistinctCounterKinds(Box::new(flt)));
            }
            ts.restore(dcp);
        }
        // `the number of <counter_kind> counters among <flt|players>` (`Kate Stewart`, `Vault 12: The Necropolis`)
        {
            let kcp = ts.checkpoint();
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("among")
            {
                if ts.eat_word("players") || ts.eat_phrase(&["all", "players"]) {
                    return Ok(ValueExpr::CounterCount {
                        subject: Box::new(SubjectRef::Player(PlayerRef::EachPlayer)),
                        kind,
                    });
                }
                if let Ok(flt) = try_parse(ts, parse_object_filter) {
                    return Ok(ValueExpr::CounterCount {
                        subject: Box::new(SubjectRef::All(Box::new(flt))),
                        kind,
                    });
                }
            }
            ts.restore(kcp);
        }
        // `the number of <flt> they've/you've/that player [has] cast this turn` (`Magebane Lizard`, `Rug of Smothering`)
        {
            let scp = ts.checkpoint();
            if let Ok(_flt) = try_parse(ts, parse_object_filter) {
                let caster = if ts.eat_phrase(&["they've", "cast", "this", "turn"])
                    || ts.eat_phrase(&["they", "cast", "this", "turn"])
                    || ts.eat_phrase(&["that", "player", "has", "cast", "this", "turn"])
                    || ts.eat_phrase(&["that", "player", "cast", "this", "turn"])
                {
                    Some(PlayerRef::ThatPlayer)
                } else if ts.eat_phrase(&["you've", "cast", "this", "turn"])
                    || ts.eat_phrase(&["you", "cast", "this", "turn"])
                {
                    Some(PlayerRef::You)
                } else {
                    None
                };
                if let Some(player) = caster {
                    return Ok(ValueExpr::EventCountThisTurn {
                        event: TurnEventKind::SpellCast,
                        player: Some(player),
                        exclude_first: false,
                    });
                }
            }
            ts.restore(scp);
        }
        // `the number of counters on <subj>` / `the number of counters it had on it`
        {
            let bcp = ts.checkpoint();
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_phrase(&["it", "had", "on", "it"])
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                    kind: crate::v2::ir::CounterKind::Any,
                });
            }
            ts.restore(bcp);
            if (ts.eat_word("counters") || ts.eat_word("counter"))
                && ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(subject),
                    kind: crate::v2::ir::CounterKind::Any,
                });
            }
            ts.restore(bcp);
        }
        if ts.eat_phrase(&["colors", "of", "mana", "spent", "to", "cast"])
            && (ts.eat_tilde() || ts.eat_word("it"))
        {
            return Ok(ValueExpr::ColorsOfManaSpent);
        }
        if ts.eat_phrase(&["creatures", "that", "convoked"]) && ts.eat_tilde() {
            return Ok(ValueExpr::ConvokedCount);
        }
        if ts.eat_phrase(&["attractions", "you've", "visited", "this", "turn"]) {
            return Ok(ValueExpr::VisitedAttractionsThisTurn);
        }
        if ts.eat_phrase(&["opponents", "you", "have"]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["cards", "in", "your", "hand"]) {
            return Ok(ValueExpr::PlayerStat {
                player: PlayerRef::You,
                stat: PlayerStatKind::CardsInHand,
            });
        }
        if ts.eat_phrase(&["cards", "looked", "at", "while", "scrying", "this", "way"]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::LookedAtScryingThisWay,
            ));
        }
        if ts.eat_phrase(&["basic", "land", "types", "among", "lands", "you", "control"]) {
            return Ok(ValueExpr::DomainCount(PlayerRef::You));
        }
        if ts.eat_phrase(&["card", "types", "among", "cards", "in"]) {
            let zone = parse_zone_ref(ts)?;
            return Ok(ValueExpr::DistinctCardTypes(zone));
        }
        if ts.eat_phrase(&["card", "types", "among"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ValueExpr::DistinctCardTypesAmong(Box::new(flt)));
        }
        // `the number of <kind> counters on <subj>` / `the number of <kind> counters <player> have/has` / `removed this way`
        {
            let ccp = ts.checkpoint();
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
            {
                if ts.eat_phrase(&["removed", "this", "way"]) {
                    return Ok(ValueExpr::PriorActionCount(
                        PriorActionKind::CountersRemovedThisWay,
                    ));
                }
                if ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Ok(ValueExpr::CounterCount {
                        subject: Box::new(subject),
                        kind,
                    });
                }
                if let Ok(player) = try_parse(ts, parse_player_ref)
                    && (ts.eat_word("have") || ts.eat_word("has"))
                {
                    return Ok(ValueExpr::PlayerCounterCount { player, kind });
                }
            }
            ts.restore(ccp);
        }
        let flt = parse_object_filter(ts)?;
        // Optional trailing `at that time` / `as you cast ~` / `as this ability resolves`
        let _ = ts.eat_phrase(&["at", "that", "time"])
            || ts.eat_phrase(&["as", "this", "ability", "resolves"])
            || (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde());
        return Ok(ValueExpr::Count(Box::new(flt)));
    }

    // `the sacrificed/exiled/destroyed/discarded <noun>'s <power|toughness|mana value>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("the") {
            let anaph = if ts.eat_word("sacrificed") {
                Some(AnaphoraRef::SacrificedObject)
            } else if ts.eat_word("exiled") {
                Some(AnaphoraRef::ExiledObject)
            } else if ts.eat_word("destroyed") {
                Some(AnaphoraRef::DestroyedObject)
            } else if ts.eat_word("discarded") {
                Some(AnaphoraRef::DiscardedObject)
            } else if ts.eat_word("revealed") {
                Some(AnaphoraRef::RevealedCard)
            } else {
                None
            };
            if let Some(a) = anaph
                && (ts.eat_word("creature's")
                    || ts.eat_word("permanent's")
                    || ts.eat_word("card's")
                    || ts.eat_word("artifact's")
                    || ts.eat_word("enchantment's")
                    || ts.eat_word("spell's"))
            {
                let stat = if ts.eat_phrase(&["mana", "value"]) {
                    Some(StatKind::ManaValue)
                } else if ts.eat_word("power") {
                    Some(StatKind::Power)
                } else if ts.eat_word("toughness") {
                    Some(StatKind::Toughness)
                } else {
                    None
                };
                if let Some(stat) = stat {
                    return Ok(ValueExpr::ObjectStat {
                        subject: Box::new(SubjectRef::Anaphora(a)),
                        stat,
                    });
                }
            }
        }
        ts.restore(cp);
    }

    // `its/their base power` / `its/their base toughness` (`Kutzil, Malamet Exemplar`, `Ms. Marvel, Elastic Ally`)
    if ts.eat_phrase(&["its", "base", "power"]) || ts.eat_phrase(&["their", "base", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["its", "base", "toughness"])
        || ts.eat_phrase(&["their", "base", "toughness"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Toughness,
        });
    }

    // `~'s power` / `~'s toughness` / `~'s loyalty` / `that creature's power` / `that creature's toughness`
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
            if ts.eat_word("loyalty") {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::SelfObject),
                    kind: crate::v2::ir::CounterKind::Loyalty,
                });
            }
        }
        ts.restore(cp);
    }
    if ts.eat_phrase(&["equipped", "creature's", "power"])
        || ts.eat_phrase(&["enchanted", "creature's", "power"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["equipped", "creature's", "toughness"])
        || ts.eat_phrase(&["enchanted", "creature's", "toughness"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature)),
            stat: StatKind::Toughness,
        });
    }
    if ts.eat_phrase(&["equipped", "creature's", "mana", "value"])
        || ts.eat_phrase(&["enchanted", "creature's", "mana", "value"])
        || ts.eat_phrase(&["enchanted", "permanent's", "mana", "value"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature)),
            stat: StatKind::ManaValue,
        });
    }
    if ts.eat_phrase(&["that", "creature's", "power"])
        || ts.eat_phrase(&["that", "creature", "card's", "power"])
        || ts.eat_phrase(&["that", "permanent's", "power"])
        || ts.eat_phrase(&["that", "spell's", "power"])
        || ts.eat_phrase(&["that", "card's", "power"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["that", "creature's", "toughness"])
        || ts.eat_phrase(&["that", "creature", "card's", "toughness"])
        || ts.eat_phrase(&["that", "permanent's", "toughness"])
        || ts.eat_phrase(&["that", "spell's", "toughness"])
        || ts.eat_phrase(&["that", "card's", "toughness"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Toughness,
        });
    }
    if ts.eat_phrase(&["that", "card's", "mana", "value"])
        || ts.eat_phrase(&["that", "creature", "card's", "mana", "value"])
        || ts.eat_phrase(&["that", "spell's", "mana", "value"])
        || ts.eat_phrase(&["that", "creature's", "mana", "value"])
        || ts.eat_phrase(&["that", "permanent's", "mana", "value"])
        || ts.eat_phrase(&["that", "enchantment's", "mana", "value"])
        || ts.eat_phrase(&["that", "artifact's", "mana", "value"])
        || ts.eat_phrase(&["that", "planeswalker's", "mana", "value"])
        || ts.eat_phrase(&["that", "saga's", "mana", "value"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCard)),
            stat: StatKind::ManaValue,
        });
    }
    // `target <noun>'s mana value/power/toughness` (`target nonland permanent's mana value`)
    {
        let tcp = ts.checkpoint();
        if ts.eat_word("target") {
            let _ = ts.eat_word("nonland");
            if ts.eat_word("permanent's")
                || ts.eat_word("creature's")
                || ts.eat_word("spell's")
                || ts.eat_word("artifact's")
                || ts.eat_word("enchantment's")
                || ts.eat_word("card's")
            {
                let stat = if ts.eat_phrase(&["mana", "value"]) {
                    Some(StatKind::ManaValue)
                } else if ts.eat_word("power") {
                    Some(StatKind::Power)
                } else if ts.eat_word("toughness") {
                    Some(StatKind::Toughness)
                } else {
                    None
                };
                if let Some(stat) = stat {
                    return Ok(ValueExpr::ObjectStat {
                        subject: Box::new(SubjectRef::TargetObject {
                            filter: Box::new(crate::v2::ir::ObjectFilter {
                                any_permanent: true,
                                ..Default::default()
                            }),
                            count: crate::v2::ir::TargetCount::Exact(1),
                            another: false,
                        }),
                        stat,
                    });
                }
            }
        }
        ts.restore(tcp);
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
    if ts.eat_phrase(&["its", "toughness"]) || ts.eat_phrase(&["their", "toughness"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Toughness,
        });
    }
    if ts.eat_phrase(&["their", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Power,
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

    // `mana from <filter> [that was] spent to cast <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["mana", "from"]) {
            let flt = parse_object_filter(ts)?;
            let _ = ts.eat_phrase(&["that", "was"]);
            if ts.eat_phrase(&["spent", "to", "cast"])
                && (ts.eat_tilde() || ts.eat_word("it") || ts.eat_word("them"))
            {
                return Ok(ValueExpr::ManaSpentFromFilter(Box::new(flt)));
            }
        }
        ts.restore(cp);
    }

    // `time you've cast your/a commander from the command zone this game` (`Empyrial Storm`, `Font of Magic`)
    if ts.eat_phrase(&["time", "you've", "cast"])
        && (ts.eat_word("your") || ts.eat_word("a"))
        && ts.eat_phrase(&[
            "commander",
            "from",
            "the",
            "command",
            "zone",
            "this",
            "game",
        ])
    {
        return Ok(ValueExpr::CommanderCastCount(PlayerRef::You));
    }
    if ts.eat_phrase(&["different", "mana", "value", "among", "cards", "in"])
        && let Ok(zref) = try_parse(ts, parse_zone_ref)
    {
        return Ok(ValueExpr::DistinctManaValues(zref));
    }
    {
        let dcp = ts.checkpoint();
        if ts.eat_word("different")
            && (ts.eat_phrase(&["mana", "value"])
                || ts.eat_word("power")
                || ts.eat_word("toughness")
                || ts.eat_phrase(&["card", "type"])
                || ts.eat_phrase(&["creature", "type"]))
            && ts.eat_word("among")
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ValueExpr::DistinctCardTypesAmong(Box::new(flt)));
        }
        ts.restore(dcp);
    }
    if ts.eat_phrase(&["counter", "on"])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(subject),
            kind: crate::v2::ir::CounterKind::Any,
        });
    }
    if ts.eat_phrase(&["counter", "among", "players", "and", "permanents"]) {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(SubjectRef::All(Box::new(crate::v2::ir::ObjectFilter {
                any_permanent: true,
                ..Default::default()
            }))),
            kind: crate::v2::ir::CounterKind::Any,
        });
    }

    // `target beyond the first` / `color of mana spent to cast ~` / `opponent you have` / `of its colors` / `card in your hand`
    if ts.eat_phrase(&["target", "beyond", "the", "first"]) {
        return Ok(ValueExpr::TargetsBeyondFirst);
    }
    if ts.eat_phrase(&["color", "of", "mana", "spent", "to", "cast"])
        && (ts.eat_tilde() || ts.eat_word("it"))
    {
        return Ok(ValueExpr::ColorsOfManaSpent);
    }
    if ts.eat_phrase(&["creature", "in", "your", "party"]) {
        return Ok(ValueExpr::PartySize(PlayerRef::You));
    }
    {
        let kcp = ts.checkpoint();
        if ts.eat_word("time")
            && (ts.eat_tilde() || ts.eat_word("it"))
            && ts.eat_phrase(&["was", "kicked"])
        {
            return Ok(ValueExpr::TimesKicked);
        }
        ts.restore(kcp);
    }
    if ts.eat_phrase(&["basic", "land", "type", "among", "lands", "you", "control"]) {
        return Ok(ValueExpr::DomainCount(PlayerRef::You));
    }
    if ts.eat_phrase(&["opponent", "you", "have"])
        || ts.eat_phrase(&["opponent", "you're", "attacking"])
        || ts.eat_phrase(&["opponent", "you", "attacked"])
        || ts.eat_phrase(&["player", "being", "attacked"])
    {
        return Ok(ValueExpr::OpponentsCount);
    }
    if ts.eat_phrase(&["card", "drawn", "this", "way"])
        || ts.eat_phrase(&["cards", "drawn", "this", "way"])
    {
        return Ok(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::CardDrawn,
            player: Some(PlayerRef::You),
            exclude_first: false,
        });
    }
    {
        let onecp = ts.checkpoint();
        if ts.next_number(false) == Some(1) {
            if ts.eat_phrase(&["damage", "prevented", "this", "way"]) {
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DamagePreventedThisWay,
                ));
            }
            if ts.eat_phrase(&["damage", "dealt"]) {
                if ts.eat_word("to") {
                    let _ = try_parse(ts, parse_subject_ref);
                }
                let _ = ts.eat_phrase(&["this", "turn"]) || ts.eat_phrase(&["this", "way"]);
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
                ));
            }
            if ts.eat_phrase(&["life", "you", "gained", "this", "turn"])
                || ts.eat_phrase(&["life", "you", "gained"])
            {
                return Ok(ValueExpr::EventCountThisTurn {
                    event: TurnEventKind::LifeGained,
                    player: Some(PlayerRef::You),
                    exclude_first: false,
                });
            }
            if ts.eat_phrase(&["life", "you", "lost", "this", "turn"])
                || ts.eat_phrase(&["life", "you", "lost"])
                || ts.eat_phrase(&["life", "lost", "this", "way"])
                || ts.eat_phrase(&["life", "your", "opponents", "have", "lost", "this", "turn"])
                || ts.eat_phrase(&["life", "your", "opponents", "lost", "this", "turn"])
            {
                return Ok(ValueExpr::EventCountThisTurn {
                    event: TurnEventKind::LifeLost,
                    player: Some(PlayerRef::You),
                    exclude_first: false,
                });
            }
        }
        ts.restore(onecp);
    }
    {
        let pcp = ts.checkpoint();
        let opp_only = if ts.eat_phrase(&["opponent", "who"]) {
            Some(true)
        } else if ts.eat_phrase(&["player", "who"]) {
            Some(false)
        } else {
            None
        };
        if let Some(opponents_only) = opp_only {
            if ts.eat_phrase(&["lost", "life", "this", "turn"]) {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event: TurnEventKind::LifeLost,
                    opponents_only,
                });
            }
            if ts.eat_phrase(&["discarded", "a", "card", "this", "turn"])
                || ts.eat_phrase(&["discarded", "one", "or", "more", "cards", "this", "turn"])
            {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event: TurnEventKind::CardDiscarded,
                    opponents_only,
                });
            }
            if ts.eat_phrase(&["drew", "two", "or", "more", "cards", "this", "turn"])
                || ts.eat_phrase(&["drew", "a", "card", "this", "turn"])
                || ts.eat_phrase(&["drew", "a", "card", "this", "way"])
                || ts.eat_phrase(&["drew", "cards", "this", "way"])
            {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event: TurnEventKind::CardDrawn,
                    opponents_only,
                });
            }
            if ts.eat_phrase(&["controls", "fewer"])
                && try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_phrase(&["than", "you"])
            {
                return Ok(ValueExpr::OpponentsCount);
            }
            if ts.eat_phrase(&["was", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["was", "dealt", "combat", "damage", "this", "turn"])
            {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event: TurnEventKind::LifeLost,
                    opponents_only,
                });
            }
            if ts.eat_phrase(&[
                "had",
                "two",
                "or",
                "more",
                "lands",
                "enter",
                "the",
                "battlefield",
                "under",
                "their",
                "control",
                "this",
                "turn",
            ]) || ts.eat_phrase(&[
                "had", "two", "or", "more", "lands", "enter", "under", "their", "control", "this",
                "turn",
            ]) {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event: TurnEventKind::SpellCast,
                    opponents_only,
                });
            }
        }
        ts.restore(pcp);
    }
    if ts.eat_phrase(&["kind", "of", "counter", "on"])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(subject),
            kind: crate::v2::ir::CounterKind::Any,
        });
    }
    if ts.eat_phrase(&["color", "among"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Group(
            Box::new(flt),
        ))));
    }
    if matches!(
        ts.peek().map(|t| &t.kind),
        Some(TokenKind::Mana(ManaSymbol::Snow))
    ) {
        let scp = ts.checkpoint();
        ts.next_tok();
        if ts.eat_phrase(&["spent", "to", "cast"]) && (ts.eat_tilde() || ts.eat_word("it")) {
            return Ok(ValueExpr::SnowManaSpent);
        }
        ts.restore(scp);
    }
    if ts.eat_phrase(&["of", "its", "colors"]) {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
            AnaphoraRef::It,
        ))));
    }
    if ts.eat_phrase(&["of", "that", "spell's", "colors"]) {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
            AnaphoraRef::ThatSpell,
        ))));
    }
    if ts.eat_phrase(&["of", "the", "sacrificed", "creature's", "colors"])
        || ts.eat_phrase(&["of", "the", "sacrificed", "permanent's", "colors"])
    {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
            AnaphoraRef::SacrificedObject,
        ))));
    }
    if ts.eat_phrase(&["of", "that", "creature's", "colors"])
        || ts.eat_phrase(&["of", "that", "permanent's", "colors"])
        || ts.eat_phrase(&["of", "the", "exiled", "card's", "colors"])
        || ts.eat_phrase(&["of", "the", "discarded", "card's", "colors"])
    {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
            AnaphoraRef::ThatObject,
        ))));
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_word("of")
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
            && ts.eat_word("'s")
            && ts.eat_word("colors")
        {
            return Ok(ValueExpr::ColorCount(Box::new(subj)));
        }
        ts.restore(ccp);
    }
    if ts.eat_phrase(&["card", "in", "your", "hand"]) {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::You,
            stat: PlayerStatKind::CardsInHand,
        });
    }
    {
        let zcp = ts.checkpoint();
        if ts.eat_phrase(&["card", "in"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                any_card: true,
                zones: vec![zref],
                ..Default::default()
            })));
        }
        ts.restore(zcp);
    }

    // `<kind> counter [on <subj> | <player> have/has | removed this way]`
    {
        let cp = ts.checkpoint();
        if let Ok(kind) = try_parse(ts, parse_counter_kind)
            && (ts.eat_word("counter") || ts.eat_word("counters"))
        {
            if ts.eat_word("on")
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(subject),
                    kind,
                });
            }
            if let Ok(player) = try_parse(ts, parse_player_ref)
                && (ts.eat_word("have") || ts.eat_word("has"))
            {
                return Ok(ValueExpr::PlayerCounterCount { player, kind });
            }
            if ts.eat_word("among")
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::Group(Box::new(flt))),
                    kind,
                });
            }
            if ts.eat_phrase(&["removed", "this", "way"])
                || ts.eat_phrase(&["put", "on", "it", "this", "way"])
            {
                return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
            }
        }
        ts.restore(cp);
    }

    // `creature exiled this way` -> Count(flt) [+ `and each <flt2>`!]
    let flt = parse_object_filter(ts)?;
    let mut expr = ValueExpr::Count(Box::new(flt));
    let acp = ts.checkpoint();
    if ts.eat_phrase(&["and", "each"])
        && let Ok(flt2) = try_parse(ts, parse_object_filter)
    {
        expr = ValueExpr::Add(Box::new(expr), Box::new(ValueExpr::Count(Box::new(flt2))));
    } else {
        ts.restore(acp);
    }
    Ok(expr)
}
