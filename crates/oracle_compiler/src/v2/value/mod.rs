//! Strictly-typed recursive [`ValueExpr`] parser for `v2`.

mod for_each;
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
pub use for_each::parse_for_each_expr;

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
    // Filter-tail fallback for `with base power and toughness <P>/<T>`
    {
        let bt_cp = ts.checkpoint();
        if ts.eat_phrase(&["and", "toughness"])
            && let Some((p, _t)) = ts.next_base_pt()
        {
            return Ok(ValueExpr::Const(p));
        }
        ts.restore(bt_cp);
    }
    // `double that damage/amount`, `an additional <N>`, `up to <N>`, `of the chosen/that quality`, `that Aura`
    if ts.eat_phrase(&["double", "that", "damage"]) || ts.eat_phrase(&["double", "that", "amount"])
    {
        return Ok(ValueExpr::Mul(
            Box::new(ValueExpr::Const(2)),
            Box::new(ValueExpr::PriorActionCount(
                PriorActionKind::DealtDamageThisWay,
            )),
        ));
    }
    {
        let acp = ts.checkpoint();
        if (ts.eat_phrase(&["an", "additional"]) || ts.eat_phrase(&["up", "to"]))
            && let Some(n) = ts.next_number(false)
        {
            return Ok(ValueExpr::Const(n));
        }
        ts.restore(acp);
    }
    if ts.eat_phrase(&["of", "the", "chosen", "quality"])
        || ts.eat_phrase(&["of", "that", "quality"])
    {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    if ts.eat_phrase(&["that", "aura"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatPermanent)),
            stat: StatKind::ManaValue,
        });
    }
    // `twice <expr>` / `<N> times <expr>` / `half <expr> [, rounded up/down]`
    if ts.eat_word("twice") {
        let rhs = parse_value_expr(ts)?;
        return Ok(ValueExpr::Mul(Box::new(ValueExpr::Const(2)), Box::new(rhs)));
    }
    if ts.eat_word("half") {
        let hcp = ts.checkpoint();
        let life_player =
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
        let expr = if let Some(p) = life_player {
            ValueExpr::PlayerStat {
                player: p,
                stat: PlayerStatKind::LifeTotal,
            }
        } else {
            ts.restore(hcp);
            parse_value_expr(ts)?
        };
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
    // `the [amount of] excess damage dealt [to <subj>] this way` / `that excess damage` / `the excess`
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
        if ts.eat_phrase(&["the", "excess"])
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("damage"))
        {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::ExcessDamageThisWay,
            ));
        }
        ts.restore(ecp);
    }
    // `the power/toughness of the creature that died` (checked BEFORE generic `the <stat> of <subj>`)
    if ts.eat_phrase(&["the", "power", "of", "the", "creature", "that", "died"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["the", "toughness", "of", "the", "creature", "that", "died"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Toughness,
        });
    }
    // `the [total] mana value/power/toughness of <subj | flt>`
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
                if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                    if ts.eat_phrase(&["goaded", "this", "way"]) {
                        flt.flags = flt.flags.union(crate::v2::ir::FilterFlags::GOADED);
                    } else if ts.eat_phrase(&["sacrificed", "this", "way"]) {
                        flt.predicates
                            .push(crate::v2::ir::FilterPredicate::AffectedByPriorAction(
                                PriorActionKind::SacrificedThisWay,
                            ));
                    } else if ts.eat_phrase(&["exiled", "this", "way"]) {
                        flt.predicates
                            .push(crate::v2::ir::FilterPredicate::AffectedByPriorAction(
                                PriorActionKind::ExiledThisWay,
                            ));
                    } else if ts.eat_tilde() && ts.eat_word("targets") {
                        flt.predicates
                            .push(crate::v2::ir::FilterPredicate::TargetsSubject(Box::new(
                                SubjectRef::SelfObject,
                            )));
                    }
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
        || (!ts.peek_phrase(&["the", "damage", "dealt", "to"])
            && ts.eat_phrase(&["the", "damage", "dealt"]))
        || (!ts.peek_phrase(&["the", "damage", "dealt"])
            && !ts.peek_phrase(&["the", "damage", "already"])
            && !ts.peek_phrase(&["the", "damage", "prevented"])
            && ts.eat_phrase(&["the", "damage"]))
    {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::DealtDamageThisWay,
        ));
    }

    // `the result` / `that result` / `the other result` / `the total of those results` / `the difference`
    // / `the chosen number` / `the last chosen number` / `that number` / `the highest/greatest number [you] noted ...`
    if ts.eat_phrase(&["the", "result"])
        || ts.eat_phrase(&["that", "result"])
        || ts.eat_phrase(&["the", "other", "result"])
        || ts.eat_phrase(&["the", "total", "of", "those", "results"])
        || ts.eat_phrase(&["the", "last", "chosen", "number"])
        || ts.eat_phrase(&["the", "chosen", "number"])
        || ts.eat_phrase(&["that", "number"])
        || (!ts.peek_phrase(&["the", "difference", "between"])
            && ts.eat_phrase(&["the", "difference"]))
    {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    {
        let ncp = ts.checkpoint();
        if (ts.eat_phrase(&["the", "highest", "number"])
            || ts.eat_phrase(&["the", "greatest", "number"]))
            && (ts.eat_phrase(&["you", "noted", "for", "cards", "named"]) && ts.eat_tilde()
                || ts.eat_phrase(&["noted", "for", "it", "this", "turn"])
                || ts.eat_phrase(&["noted", "for", "cards", "named"]) && ts.eat_tilde()
                || ts.eat_phrase(&[
                    "of", "stored", "results", "on", "it", "of", "the", "same", "value",
                ]))
        {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        ts.restore(ncp);
    }
    if ts.eat_phrase(&["the", "highest", "number"]) || ts.eat_phrase(&["the", "lowest", "number"]) {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    if ts.eat_phrase(&[
        "the", "total", "life", "lost", "by", "all", "players", "this", "turn",
    ]) {
        return Ok(ValueExpr::EventCountThisTurn {
            event: TurnEventKind::LifeLost,
            player: Some(PlayerRef::EachPlayer),
            exclude_first: false,
        });
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
    ]) || ts.eat_phrase(&[
        "the",
        "total",
        "life",
        "lost",
        "by",
        "your",
        "opponents",
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
    if ts.eat_phrase(&["each", "other", "player's", "speed"]) {
        return Ok(ValueExpr::Speed(PlayerRef::EachOtherPlayer));
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
    ]) || ts.eat_phrase(&[
        "the",
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

    // `your starting life total` / `your life total` / `that player's life total` / `your devotion to <colors>`
    if ts.eat_phrase(&["your", "starting", "life", "total"])
        || ts.eat_phrase(&["your", "life", "total"])
    {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::You,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if ts.eat_phrase(&["that", "player's", "life", "total"])
        || ts.eat_phrase(&["that", "opponent's", "life", "total"])
        || ts.eat_phrase(&["their", "life", "total"])
    {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::ThatPlayer,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if ts.eat_phrase(&["target", "player's", "life", "total"]) {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::TargetPlayer,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    if ts.eat_phrase(&["target", "opponent's", "life", "total"]) {
        return Ok(ValueExpr::PlayerStat {
            player: PlayerRef::TargetOpponent,
            stat: PlayerStatKind::LifeTotal,
        });
    }
    {
        let ncp = ts.checkpoint();
        if ts.eat_phrase(&["the", "last", "noted", "life", "total", "for"]) && ts.eat_tilde() {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        ts.restore(ncp);
    }
    {
        let rcp = ts.checkpoint();
        if ts.eat_phrase(&["your", "ring-bearer's"]) {
            let stat = if ts.eat_word("power") {
                Some(StatKind::Power)
            } else if ts.eat_word("toughness") {
                Some(StatKind::Toughness)
            } else if ts.eat_phrase(&["mana", "value"]) {
                Some(StatKind::ManaValue)
            } else {
                None
            };
            if let Some(stat) = stat {
                return Ok(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
                    stat,
                });
            }
        }
        ts.restore(rcp);
    }
    if ts.eat_phrase(&["your", "devotion", "to"]) {
        if ts.eat_phrase(&["that", "color"]) || ts.eat_phrase(&["the", "chosen", "color"]) {
            return Ok(ValueExpr::Devotion(crate::ast::ColorMask::EMPTY));
        }
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
                if let Ok(player) = try_parse(ts, parse_player_ref)
                    && (ts.eat_word("have") || ts.eat_word("has"))
                {
                    return Ok(ValueExpr::PlayerCounterCount {
                        player,
                        kind: crate::v2::ir::CounterKind::Energy,
                    });
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
            if ts.eat_phrase(&["generic", "mana", "in", "that", "spell's", "mana", "cost"]) {
                return Ok(ValueExpr::ObjectStat {
                    subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatSpell)),
                    stat: StatKind::ManaValue,
                });
            }
            if ts.eat_phrase(&["generic", "mana", "in"])
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["'s", "mana", "cost"])
            {
                return Ok(ValueExpr::ObjectStat {
                    subject: Box::new(subj),
                    stat: StatKind::ManaValue,
                });
            }
            if ts.eat_phrase(&["unspent", "mana", "you", "have"]) {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
            if ts.eat_phrase(&["damage", "dealt"]) {
                if ts.eat_word("to") {
                    let _ = try_parse(ts, parse_subject_ref);
                }
                let _ = ts.eat_phrase(&["this", "way"]) || ts.eat_phrase(&["this", "turn"]);
                if ts.eat_word("by") {
                    let _ = try_parse(ts, parse_object_filter);
                }
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
                || ts.eat_phrase(&["life", "total", "among", "your", "opponents"])
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
                && ts.eat_phrase(&[
                    "amount",
                    "of",
                    "damage",
                    "dealt",
                    "by",
                    "a",
                    "source",
                    "to",
                    "a",
                    "permanent",
                    "or",
                    "player",
                    "this",
                    "turn",
                ])
            {
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
                ));
            }
            if stat.is_none()
                && ts.eat_phrase(&["number", "of"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                let _ = ts.eat_phrase(&["that", "have", "a", "creature", "type", "in", "common"]);
                return Ok(ValueExpr::Count(Box::new(flt)));
            }
            if let Some(stat) = stat
                && (ts.eat_word("among") || ts.eat_word("of"))
            {
                if ts.eat_word("them") || ts.eat_phrase(&["the", "milled", "cards"]) {
                    return Ok(ValueExpr::AggregateStat {
                        op,
                        stat,
                        filter: Box::new(crate::v2::ir::ObjectFilter {
                            any_permanent: true,
                            ..Default::default()
                        }),
                    });
                }
                let _ = ts.eat_word("those") || ts.eat_word("the");
                if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                    let acp = ts.checkpoint();
                    if ts.eat_word("and")
                        && let Ok(other) = try_parse(ts, parse_object_filter)
                    {
                        flt = crate::v2::ir::ObjectFilter {
                            predicates: vec![crate::v2::ir::FilterPredicate::AnyOf(vec![
                                flt, other,
                            ])],
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
    if ts.eat_phrase(&[
        "the",
        "number",
        "of",
        "colors",
        "in",
        "your",
        "commanders'",
        "color",
        "identity",
    ]) || ts.eat_phrase(&[
        "the",
        "number",
        "of",
        "colors",
        "in",
        "your",
        "commander's",
        "color",
        "identity",
    ]) {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Group(
            Box::new(crate::v2::ir::ObjectFilter {
                flags: crate::v2::ir::FilterFlags::IS_COMMANDER,
                owner: Some(crate::ast::ControllerRelation::OwnerYou),
                ..Default::default()
            }),
        ))));
    }
    if ts.eat_phrase(&["the", "amassed", "army's", "power"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCreature)),
            stat: StatKind::Power,
        });
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
    // `the life total of an opponent with the most life`
    if ts.eat_phrase(&[
        "the", "life", "total", "of", "an", "opponent", "with", "the", "most", "life",
    ]) || ts.eat_phrase(&[
        "the", "life", "total", "of", "a", "player", "with", "the", "most", "life",
    ]) {
        return Ok(ValueExpr::HighestLifeTotal);
    }
    // `the damage [already] dealt to <player|subj> [so far] this turn [by <flt>]`
    {
        let dcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "damage"]) {
            let _ = ts.eat_word("already");
            if ts.eat_phrase(&["dealt", "to"]) {
                let p_opt = try_parse(ts, parse_player_ref).ok();
                let s_opt = if p_opt.is_none() {
                    try_parse(ts, parse_subject_ref).ok()
                } else {
                    None
                };
                if p_opt.is_some() || s_opt.is_some() {
                    let _ = ts.eat_phrase(&["so", "far"]);
                    if ts.eat_phrase(&["this", "turn"]) {
                        if ts.eat_word("by") {
                            let _ = try_parse(ts, parse_object_filter);
                        }
                        return Ok(if let Some(player) = p_opt {
                            ValueExpr::DamageDealtToPlayerThisTurn(player)
                        } else {
                            ValueExpr::PriorActionCount(PriorActionKind::DealtDamageThisWay)
                        });
                    }
                }
            }
        }
        ts.restore(dcp);
    }
    // `the difference between <subj>'s power and [its] toughness` OR `the difference between those players' life totals` / `the chosen creatures' powers` OR `the difference between <val1> and <val2>`
    {
        let diff_cp = ts.checkpoint();
        if ts.eat_phrase(&["the", "difference", "between"]) {
            if ts.eat_phrase(&["those", "players'", "life", "totals"]) {
                return Ok(ValueExpr::StatDifference(Box::new(SubjectRef::Player(
                    PlayerRef::EachPlayer,
                ))));
            }
            if ts.eat_phrase(&["the", "chosen", "creatures'", "powers"]) {
                return Ok(ValueExpr::StatDifference(Box::new(SubjectRef::Anaphora(
                    AnaphoraRef::ThoseCreatures,
                ))));
            }
            let inner_cp = ts.checkpoint();
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
            ts.restore(inner_cp);
            if let Ok(v1) = try_parse(ts, parse_value_expr)
                && ts.eat_word("and")
                && let Ok(v2) = try_parse(ts, parse_value_expr)
            {
                return Ok(ValueExpr::Sub(Box::new(v1), Box::new(v2)));
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
    // `[the total | the] number of ...`
    if ts.eat_phrase(&["the", "total", "number", "of"]) || ts.eat_phrase(&["the", "number", "of"]) {
        if ts.eat_phrase(&["creatures", "that", "died", "this", "way"]) {
            return Ok(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                any_types: crate::ast::CardTypeMask::CREATURE,
                predicates: vec![crate::v2::ir::FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::DestroyedThisWay,
                )],
                ..Default::default()
            })));
        }
        if ts.eat_phrase(&[
            "times", "it's", "been", "cast", "from", "the", "command", "zone", "this", "game",
        ]) {
            return Ok(ValueExpr::CommanderCastCount(PlayerRef::You));
        }
        if ts.eat_phrase(&["counters", "removed", "this", "way"]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::CountersRemovedThisWay,
            ));
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_phrase(&["counters", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::All(Box::new(flt))),
                    kind: crate::v2::ir::CounterKind::Any,
                });
            }
            ts.restore(acp);
        }
        if ts.eat_phrase(&[
            "cards",
            "in",
            "your",
            "graveyard",
            "that",
            "are",
            "instant",
            "cards",
        ]) || ts.eat_phrase(&[
            "cards",
            "you",
            "own",
            "in",
            "exile",
            "and",
            "in",
            "your",
            "graveyard",
            "that",
            "are",
            "instant",
            "cards",
        ]) {
            let _ = ts.eat_comma();
            let _ = ts.eat_word("are");
            if ts.eat_phrase(&["sorcery", "cards"]) {
                let _ = ts.eat_comma();
                if ts.eat_phrase(&["and/or", "have", "an", "adventure"]) {
                    return Ok(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                        any_types: crate::ast::CardTypeMask::INSTANT
                            .union(crate::ast::CardTypeMask::SORCERY),
                        flags: crate::v2::ir::FilterFlags::HAS_ADVENTURE,
                        any_card: true,
                        zones: vec![crate::v2::ir::ZoneRef {
                            zone: crate::v2::ir::ZoneKind::Graveyard,
                            owner: crate::v2::ir::ZoneOwner::You,
                        }],
                        ..Default::default()
                    })));
                }
            }
        }
        if ts.eat_phrase(&["permanents", "you've", "sacrificed", "this", "turn"])
            || ts.eat_phrase(&["permanents", "you", "sacrificed", "this", "turn"])
        {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::SacrificedThisWay,
            ));
        }
        if ts.eat_phrase(&["tokens", "you", "created", "this", "turn"])
            || ts.eat_phrase(&["tokens", "you've", "created", "this", "turn"])
            || ts.eat_phrase(&["treasure", "tokens", "you", "created", "this", "turn"])
            || ts.eat_phrase(&["treasure", "tokens", "you've", "created", "this", "turn"])
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
        if ts.eat_phrase(&["unique", "vowels", "on", "that", "sticker"])
            || ts.eat_phrase(&["times", "you", "descended", "this", "turn"])
            || ts.eat_phrase(&["coins", "that", "came", "up", "heads"])
            || ts.eat_phrase(&["times", "you", "chose", "a", "mode", "for", "that", "spell"])
            || (ts.eat_phrase(&[
                "cards", "you", "removed", "from", "the", "draft", "with", "cards", "named",
            ]) && ts.eat_tilde())
        {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        if ts.eat_phrase(&["opponents", "being", "attacked"])
            || ts.eat_phrase(&["opponents", "you", "attacked", "this", "turn"])
            || ts.eat_phrase(&[
                "opponents",
                "who",
                "have",
                "three",
                "or",
                "more",
                "poison",
                "counters",
            ])
            || ts.eat_phrase(&[
                "your",
                "opponents",
                "with",
                "four",
                "or",
                "more",
                "cards",
                "in",
                "hand",
            ])
        {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["unlocked", "doors", "among", "rooms", "you", "control"]) {
            return Ok(ValueExpr::Count(Box::new(crate::v2::ir::ObjectFilter {
                subtypes: vec!["Room".to_string()],
                ..Default::default()
            })));
        }
        if ts.eat_phrase(&[
            "planeswalker",
            "types",
            "among",
            "planeswalkers",
            "you",
            "control",
        ]) || (ts.eat_word("supertypes")
            && ts.eat_comma()
            && ts.eat_phrase(&["card", "types"])
            && ts.eat_comma()
            && ts.eat_phrase(&["and", "subtypes", "it", "has"]))
            || ts.eat_phrase(&["card", "types", "the", "discarded", "card", "has"])
        {
            return Ok(ValueExpr::DistinctCardTypesAmong(Box::default()));
        }
        {
            let pcp = ts.checkpoint();
            if ts.eat_phrase(&["different", "color", "pairs", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                let _ = ts.eat_phrase(&["that", "are", "exactly", "two", "colors"]);
                return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Group(
                    Box::new(flt),
                ))));
            }
            ts.restore(pcp);
            if ts.eat_phrase(&["different", "powers", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::DistinctCardTypesAmong(Box::new(flt)));
            }
            ts.restore(pcp);
            // `<letter>'s in name stickers on ~` (`_____ Balls of Fire`, `Make a _____ Splash`)
            if let Some(w) = ts.peek_word()
                && w.ends_with("'s")
                && w.len() == 3
            {
                ts.next_word();
                if ts.eat_phrase(&["in", "name", "stickers", "on"]) && ts.eat_tilde() {
                    return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
                }
            }
            ts.restore(pcp);
            // `abilities from among <kw>, <kw>, ..., and <kw> [found] among creatures you control`
            if ts.eat_phrase(&["abilities", "from", "among"]) {
                let mut saw_kw = false;
                while try_parse(ts, crate::v2::filter::parse_keyword_kind).is_ok() {
                    saw_kw = true;
                    let _ = ts.eat_comma();
                    let _ = ts.eat_word("and");
                }
                let _ = ts.eat_word("found");
                if saw_kw && ts.eat_word("among") && try_parse(ts, parse_object_filter).is_ok() {
                    return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
                }
            }
            ts.restore(pcp);
            if ts.eat_phrase(&["items", "they", "revealed"]) {
                return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
            }
            // `cards you own in exile and in your graveyard that are <flt> or are named ~` (`Slime Against Humanity`)
            if ts.eat_phrase(&[
                "cards",
                "you",
                "own",
                "in",
                "exile",
                "and",
                "in",
                "your",
                "graveyard",
                "that",
                "are",
            ]) && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                let _ = ts.eat_phrase(&["or", "are", "named"]) && ts.eat_tilde();
                return Ok(ValueExpr::Count(Box::new(flt)));
            }
            ts.restore(pcp);
        }
        if ts.eat_phrase(&["colors", "among"]) {
            if let Ok(flt) = try_parse(ts, parse_object_filter) {
                let _ = ts.eat_phrase(&["and", "spells", "you've", "cast", "this", "turn"]);
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
                && (ts.eat_word("is")
                    || ts.eat_word("are")
                    || ts.eat_word("was")
                    || ts.eat_word("were"))
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
        if ts.eat_phrase(&["card", "types", "among", "cards", "in"])
            || ts.eat_phrase(&["permanent", "types", "among", "cards", "in"])
        {
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
        let mut flt = parse_object_filter(ts)?;
        if ts.eat_phrase(&[
            "that",
            "entered",
            "the",
            "battlefield",
            "under",
            "your",
            "control",
            "this",
            "turn",
        ]) || ts.eat_phrase(&[
            "that", "entered", "under", "your", "control", "this", "turn",
        ]) {
            flt.flags = flt
                .flags
                .union(crate::v2::ir::FilterFlags::ENTERED_THIS_TURN);
            flt.controller = Some(crate::ast::ControllerRelation::You);
        }
        if ts.eat_phrase(&["that", "died", "under", "your", "control", "this", "turn"])
            || ts.eat_phrase(&["that", "died", "this", "turn"])
        {
            flt.flags = flt.flags.union(crate::v2::ir::FilterFlags::DIED_THIS_TURN);
        }
        let _ = ts.eat_phrase(&[
            "controlled",
            "by",
            "the",
            "player",
            "who",
            "controls",
            "the",
            "fewest",
        ]) || ts.eat_phrase(&["those", "players", "control"])
            || ts.eat_phrase(&["that", "player", "controls"]);
        // Optional trailing `at that time` / `as you cast ~` / `as this ability resolves`
        let _ = ts.eat_phrase(&["at", "that", "time"])
            || ts.eat_phrase(&["as", "this", "ability", "resolves"])
            || (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde());
        return Ok(ValueExpr::Count(Box::new(flt)));
    }

    // `the [tapped/sacrificed/exiled/...] <noun>'s <power|toughness|mana value>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("the") || ts.eat_word("that") {
            let anaph = if ts.eat_word("sacrificed") {
                AnaphoraRef::SacrificedObject
            } else if ts.eat_word("exiled") {
                AnaphoraRef::ExiledObject
            } else if ts.eat_word("destroyed") {
                AnaphoraRef::DestroyedObject
            } else if ts.eat_word("discarded") {
                AnaphoraRef::DiscardedObject
            } else if ts.eat_word("revealed") {
                AnaphoraRef::RevealedCard
            } else {
                let _ = ts.eat_word("tapped")
                    || ts.eat_word("attacking")
                    || ts.eat_word("blocking")
                    || ts.eat_word("chosen");
                AnaphoraRef::ThatObject
            };
            let _ = ts.eat_word("creature")
                || ts.eat_word("nonland")
                || ts.eat_word("instant")
                || ts.eat_word("sorcery");
            if let Some(noun) = ts.peek_word()
                && noun.ends_with("'s")
            {
                ts.next_word();
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
                        subject: Box::new(SubjectRef::Anaphora(anaph)),
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
        || ts.eat_phrase(&["that", "nonland", "card's", "mana", "value"])
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

    // `its mana value` / `its power` / `its toughness` / `its loyalty` / `his/her power/toughness`
    if ts.eat_phrase(&["its", "loyalty"]) {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            kind: crate::v2::ir::CounterKind::Loyalty,
        });
    }
    if ts.eat_phrase(&["its", "mana", "value"]) {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::ManaValue,
        });
    }
    if ts.eat_phrase(&["its", "power"])
        || ts.eat_phrase(&["his", "power"])
        || ts.eat_phrase(&["her", "power"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            stat: StatKind::Power,
        });
    }
    if ts.eat_phrase(&["its", "toughness"])
        || ts.eat_phrase(&["his", "toughness"])
        || ts.eat_phrase(&["her", "toughness"])
        || ts.eat_phrase(&["their", "toughness"])
    {
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
