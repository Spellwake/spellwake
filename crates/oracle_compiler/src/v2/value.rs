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
    // `the mana value/power/toughness of <subj>` (`the mana value of the exiled card`, `the power of the chosen creature`)
    {
        let ocp = ts.checkpoint();
        if ts.eat_word("the") {
            let stat = if ts.eat_phrase(&["mana", "value", "of"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_phrase(&["power", "of"]) {
                Some(StatKind::Power)
            } else if ts.eat_phrase(&["toughness", "of"]) {
                Some(StatKind::Toughness)
            } else {
                None
            };
            if let Some(stat) = stat
                && let Ok(subject) = try_parse(ts, parse_subject_ref)
            {
                return Ok(ValueExpr::ObjectStat {
                    subject: Box::new(subject),
                    stat,
                });
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
    if ts.eat_phrase(&["the", "result"]) {
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
        let flt = parse_object_filter(ts)?;
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(flt),
        });
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
            if ts.eat_phrase(&["damage", "dealt"]) {
                let _ = ts.eat_phrase(&["this", "way"]) || ts.eat_phrase(&["this", "turn"]);
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
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
        if ts.eat_phrase(&["colors", "among"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Group(
                Box::new(flt),
            ))));
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
        }
        // `the number of counters on <subj>` (bare unspecified counter kind! `Warden of the Grove`, `Parting Thoughts`)
        {
            let bcp = ts.checkpoint();
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
        if ts.eat_phrase(&["opponents", "you", "have"]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["cards", "in", "your", "hand"]) {
            return Ok(ValueExpr::PlayerStat {
                player: PlayerRef::You,
                stat: PlayerStatKind::CardsInHand,
            });
        }
        if ts.eat_phrase(&["basic", "land", "types", "among", "lands", "you", "control"]) {
            return Ok(ValueExpr::DomainCount(PlayerRef::You));
        }
        if ts.eat_phrase(&["card", "types", "among", "cards", "in"]) {
            let zone = parse_zone_ref(ts)?;
            return Ok(ValueExpr::DistinctCardTypes(zone));
        }
        // `the number of <kind> counters on <subj>` / `the number of <kind> counters <player> have/has`
        {
            let ccp = ts.checkpoint();
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counters") || ts.eat_word("counter"))
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
            }
            ts.restore(ccp);
        }
        let flt = parse_object_filter(ts)?;
        // Optional trailing `at that time` (e.g. delayed trigger X binding)
        let _ = ts.eat_phrase(&["at", "that", "time"]);
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
        || ts.eat_phrase(&["that", "spell's", "mana", "value"])
        || ts.eat_phrase(&["that", "creature's", "mana", "value"])
        || ts.eat_phrase(&["that", "permanent's", "mana", "value"])
    {
        return Ok(ValueExpr::ObjectStat {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::ThatCard)),
            stat: StatKind::ManaValue,
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

    // `time you've cast your commander from the command zone this game` (`Empyrial Storm`)
    if ts.eat_phrase(&[
        "time",
        "you've",
        "cast",
        "your",
        "commander",
        "from",
        "the",
        "command",
        "zone",
        "this",
        "game",
    ]) {
        return Ok(ValueExpr::CommanderCastCount(PlayerRef::You));
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
    if ts.eat_phrase(&["opponent", "you", "have"]) {
        return Ok(ValueExpr::OpponentsCount);
    }
    if ts.eat_phrase(&["of", "its", "colors"]) {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(
            AnaphoraRef::It,
        ))));
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
            if ts.eat_phrase(&["removed", "this", "way"])
                || ts.eat_phrase(&["put", "on", "it", "this", "way"])
            {
                return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
            }
        }
        ts.restore(cp);
    }

    // `creature exiled this way` -> PriorActionCount(ExiledThisWay) or Count(flt)
    let flt = parse_object_filter(ts)?;
    Ok(ValueExpr::Count(Box::new(flt)))
}
