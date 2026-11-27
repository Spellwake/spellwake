//! Strictly-typed recursive [`ValueExpr`] parser for `v2`.

mod for_each;
use crate::ast::{CardTypeMask, ColorMask, ControllerRelation, PlayerStatKind, Rounding, StatKind};
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AggregateOp, AnaphoraRef, CounterKind, FilterFlags, FilterPredicate, ManaSymbol, ObjectFilter,
    PlayerRef, PriorActionKind, SubjectRef, TargetCount, TurnEventKind, ValueExpr, VarName,
    ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
pub use for_each::parse_for_each_expr;

fn eat_table<T: Copy>(ts: &mut TokenStream<'_>, table: &[(&[&str], T)]) -> Option<T> {
    table
        .iter()
        .find_map(|&(phrase, val)| ts.eat_phrase(phrase).then_some(val))
}

fn parse_stat_word(ts: &mut TokenStream<'_>) -> Option<StatKind> {
    if ts.eat_phrase(&["mana", "value"]) {
        Some(StatKind::ManaValue)
    } else if ts.eat_word("power") {
        Some(StatKind::Power)
    } else if ts.eat_word("toughness") {
        Some(StatKind::Toughness)
    } else {
        None
    }
}

fn obj_stat(subject: SubjectRef, stat: StatKind) -> ValueExpr {
    ValueExpr::ObjectStat {
        subject: Box::new(subject),
        stat,
    }
}

fn player_stat(player: PlayerRef, stat: PlayerStatKind) -> ValueExpr {
    ValueExpr::PlayerStat { player, stat }
}

fn turn_event(event: TurnEventKind, player: Option<PlayerRef>) -> ValueExpr {
    ValueExpr::EventCountThisTurn {
        event,
        player,
        exclude_first: false,
    }
}

fn any_perm_agg(op: AggregateOp, stat: StatKind) -> ValueExpr {
    ValueExpr::AggregateStat {
        op,
        stat,
        filter: Box::new(ObjectFilter {
            any_permanent: true,
            ..Default::default()
        }),
    }
}

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
        return Ok(obj_stat(
            SubjectRef::Anaphora(AnaphoraRef::ThatPermanent),
            StatKind::ManaValue,
        ));
    }
    // `twice <expr>` / `<N> times <expr>` / `half <expr> [, rounded up/down]`
    if ts.eat_word("twice") {
        let rhs = parse_value_expr(ts)?;
        return Ok(ValueExpr::Mul(Box::new(ValueExpr::Const(2)), Box::new(rhs)));
    }
    if ts.eat_word("half") {
        const HALF_LIFE_PLAYERS: &[(&[&str], PlayerRef)] = &[
            (&["your", "life", "total"], PlayerRef::You),
            (&["your", "life"], PlayerRef::You),
            (&["their", "life", "total"], PlayerRef::ThatPlayer),
            (&["their", "life"], PlayerRef::ThatPlayer),
            (
                &["that", "player's", "life", "total"],
                PlayerRef::ThatPlayer,
            ),
            (&["that", "player's", "life"], PlayerRef::ThatPlayer),
            (
                &["target", "player's", "life", "total"],
                PlayerRef::TargetPlayer,
            ),
            (&["target", "player's", "life"], PlayerRef::TargetPlayer),
            (
                &["target", "opponent's", "life", "total"],
                PlayerRef::TargetOpponent,
            ),
            (&["target", "opponent's", "life"], PlayerRef::TargetOpponent),
        ];
        let hcp = ts.checkpoint();
        let expr = if let Some(p) = eat_table(ts, HALF_LIFE_PLAYERS) {
            player_stat(p, PlayerStatKind::LifeTotal)
        } else {
            ts.restore(hcp);
            parse_value_expr(ts)?
        };
        let rounding = match ts.eat_rounded_modifier() {
            Some(true) => Rounding::Up,
            Some(false) => Rounding::Down,
            None => {
                let _ = ts.eat_comma();
                Rounding::Down
            }
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
        return Ok(obj_stat(
            SubjectRef::Anaphora(AnaphoraRef::ThatCreature),
            StatKind::Power,
        ));
    }
    if ts.eat_phrase(&["the", "toughness", "of", "the", "creature", "that", "died"]) {
        return Ok(obj_stat(
            SubjectRef::Anaphora(AnaphoraRef::ThatCreature),
            StatKind::Toughness,
        ));
    }
    // `the [total] mana value/power/toughness of <subj | flt>`
    {
        let ocp = ts.checkpoint();
        if ts.eat_word("the") {
            let is_total = ts.eat_word("total");
            if let Some(stat) = parse_stat_word(ts)
                && ts.eat_word("of")
            {
                if !is_total {
                    // Pick the longer parse between `parse_subject_ref` and `the + parse_object_filter`
                    // so `the toughness of the card returned this way` doesn't stop after `the card`!
                    let best_cp = ts.checkpoint();
                    let subj_res = try_parse(ts, parse_subject_ref).ok();
                    let subj_pos = ts.pos;
                    ts.restore(best_cp);
                    let flt_res = if ts.eat_any_word(&["the", "all"]) {
                        try_parse(ts, parse_object_filter).ok()
                    } else {
                        None
                    };
                    let flt_pos = ts.pos;
                    if let Some(flt) = flt_res
                        && flt_pos > subj_pos
                    {
                        return Ok(obj_stat(SubjectRef::Group(Box::new(flt)), stat));
                    }
                    if let Some(subject) = subj_res {
                        ts.pos = subj_pos;
                        return Ok(obj_stat(subject, stat));
                    }
                    ts.restore(best_cp);
                }
                let _ = ts.eat_any_word(&["all", "the", "those"]);
                if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                    if ts.eat_phrase(&["goaded", "this", "way"]) {
                        flt.flags = flt.flags.union(FilterFlags::GOADED);
                    } else if ts.eat_phrase(&["sacrificed", "this", "way"]) {
                        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                            PriorActionKind::SacrificedThisWay,
                        ));
                    } else if ts.eat_phrase(&["exiled", "this", "way"]) {
                        flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                            PriorActionKind::ExiledThisWay,
                        ));
                    } else if ts.eat_tilde() && ts.eat_word("targets") {
                        flt.predicates
                            .push(FilterPredicate::TargetsSubject(Box::new(
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
    if ts.eat_any_phrase(&[
        &["the", "damage", "prevented", "this", "way"],
        &["the", "amount", "of", "damage", "prevented", "this", "way"],
    ]) {
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
        return Ok(any_perm_agg(AggregateOp::Sum, StatKind::Power));
    }
    if ts.eat_phrase(&["their", "total", "toughness"]) {
        return Ok(any_perm_agg(AggregateOp::Sum, StatKind::Toughness));
    }
    if ts.eat_any_phrase(&[
        &["the", "life", "lost", "this", "way"],
        &["the", "life", "gained", "this", "way"],
        &["the", "damage", "dealt", "this", "way"],
    ]) || (!ts.peek_phrase(&["the", "damage", "dealt", "to"])
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
    if ts.eat_any_phrase(&[
        &["the", "result"],
        &["that", "result"],
        &["the", "other", "result"],
        &["the", "total", "of", "those", "results"],
        &["the", "last", "chosen", "number"],
        &["the", "chosen", "number"],
        &["that", "number"],
    ]) || (!ts.peek_phrase(&["the", "difference", "between"])
        && ts.eat_phrase(&["the", "difference"]))
    {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    {
        let ncp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["the", "highest", "number"],
            &["the", "greatest", "number"],
        ]) && (ts.eat_phrase(&["you", "noted", "for", "cards", "named"]) && ts.eat_tilde()
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
    if ts.eat_any_phrase(&[&["the", "highest", "number"], &["the", "lowest", "number"]]) {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    if ts.eat_phrase(&[
        "the", "total", "life", "lost", "by", "all", "players", "this", "turn",
    ]) {
        return Ok(turn_event(
            TurnEventKind::LifeLost,
            Some(PlayerRef::EachPlayer),
        ));
    }
    if ts.eat_any_phrase(&[
        &[
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
        ],
        &[
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
        ],
        &[
            "the",
            "total",
            "life",
            "lost",
            "by",
            "your",
            "opponents",
            "this",
            "turn",
        ],
    ]) {
        return Ok(turn_event(
            TurnEventKind::LifeLost,
            Some(PlayerRef::EachOpponent),
        ));
    }
    const SPEED_TABLE: &[(&[&str], PlayerRef)] = &[
        (&["your", "speed"], PlayerRef::You),
        (
            &["each", "other", "player's", "speed"],
            PlayerRef::EachOtherPlayer,
        ),
        (&["that", "player's", "speed"], PlayerRef::ThatPlayer),
        (&["that", "opponent's", "speed"], PlayerRef::ThatPlayer),
        (&["their", "speed"], PlayerRef::ThatPlayer),
    ];
    if let Some(player) = eat_table(ts, SPEED_TABLE) {
        return Ok(ValueExpr::Speed(player));
    }
    if ts.eat_any_phrase(&[
        &[
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
        ],
        &[
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
        ],
    ]) {
        return Ok(ValueExpr::DamageDealtToOpponentsThisTurn {
            noncombat_only: true,
        });
    }
    if ts.eat_any_phrase(&[
        &[
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
        ],
        &[
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
        ],
        &[
            "the",
            "damage",
            "dealt",
            "to",
            "your",
            "opponents",
            "this",
            "turn",
        ],
    ]) {
        return Ok(ValueExpr::DamageDealtToOpponentsThisTurn {
            noncombat_only: false,
        });
    }
    if ts.eat_any_phrase(&[
        &[
            "the", "total", "amount", "of", "mana", "paid", "this", "way",
        ],
        &["the", "amount", "of", "mana", "paid", "this", "way"],
    ]) {
        return Ok(ValueExpr::ManaPaidThisWay);
    }

    // `your starting life total` / `your life total` / `that player's life total` / `your devotion to <colors>`
    const LIFE_TOTAL_TABLE: &[(&[&str], PlayerRef)] = &[
        (&["your", "starting", "life", "total"], PlayerRef::You),
        (&["your", "life", "total"], PlayerRef::You),
        (
            &["that", "player's", "life", "total"],
            PlayerRef::ThatPlayer,
        ),
        (
            &["that", "opponent's", "life", "total"],
            PlayerRef::ThatPlayer,
        ),
        (&["their", "life", "total"], PlayerRef::ThatPlayer),
        (
            &["target", "player's", "life", "total"],
            PlayerRef::TargetPlayer,
        ),
        (
            &["target", "opponent's", "life", "total"],
            PlayerRef::TargetOpponent,
        ),
    ];
    if let Some(player) = eat_table(ts, LIFE_TOTAL_TABLE) {
        return Ok(player_stat(player, PlayerStatKind::LifeTotal));
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
        if ts.eat_phrase(&["your", "ring-bearer's"])
            && let Some(stat) = parse_stat_word(ts)
        {
            return Ok(obj_stat(
                SubjectRef::Anaphora(AnaphoraRef::ThatCreature),
                stat,
            ));
        }
        ts.restore(rcp);
    }
    if ts.eat_phrase(&["your", "devotion", "to"]) {
        if ts.eat_any_phrase(&[&["that", "color"], &["the", "chosen", "color"]]) {
            return Ok(ValueExpr::Devotion(ColorMask::EMPTY));
        }
        let mut mask = ColorMask::EMPTY;
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
                    && ts.eat_any_word(&["have", "has"])
                {
                    return Ok(ValueExpr::PlayerCounterCount {
                        player,
                        kind: CounterKind::Energy,
                    });
                }
            }
            if ts.eat_any_phrase(&[
                &["life", "you", "gained"],
                &["life", "you've", "gained"],
                &["life", "they", "gained"],
                &["life", "they've", "gained"],
                &["life", "that", "player", "gained"],
                &["life", "gained"],
            ]) {
                let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "way"]]);
                return Ok(turn_event(TurnEventKind::LifeGained, Some(PlayerRef::You)));
            }
            if ts.eat_any_phrase(&[
                &["life", "you", "lost"],
                &["life", "you've", "lost"],
                &["life", "they", "lost"],
                &["life", "they've", "lost"],
                &["life", "that", "player", "lost"],
                &["life", "lost"],
                &["life", "your", "opponents", "lost"],
            ]) {
                let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "way"]]);
                return Ok(turn_event(TurnEventKind::LifeLost, Some(PlayerRef::You)));
            }
            if ts.eat_phrase(&["mana", "spent", "to", "cast"])
                && try_parse(ts, parse_subject_ref).is_ok()
            {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
            if ts.eat_phrase(&["generic", "mana", "in", "that", "spell's", "mana", "cost"]) {
                return Ok(obj_stat(
                    SubjectRef::Anaphora(AnaphoraRef::ThatSpell),
                    StatKind::ManaValue,
                ));
            }
            if ts.eat_phrase(&["generic", "mana", "in"])
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["'s", "mana", "cost"])
            {
                return Ok(obj_stat(subj, StatKind::ManaValue));
            }
            if ts.eat_phrase(&["unspent", "mana", "you", "have"]) {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
            if ts.eat_phrase(&["damage", "dealt"]) {
                if ts.eat_word("to") {
                    let _ = try_parse(ts, parse_subject_ref);
                }
                let _ = ts.eat_any_phrase(&[&["this", "way"], &["this", "turn"]]);
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
                let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "way"]]);
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
        let agg = if ts.eat_any_phrase(&[&["the", "greatest"], &["the", "highest"]]) {
            Some(AggregateOp::Max)
        } else if ts.eat_any_phrase(&[&["the", "least"], &["the", "lowest"]]) {
            Some(AggregateOp::Min)
        } else {
            None
        };
        if let Some(op) = agg {
            if ts.eat_any_phrase(&[
                &["life", "total", "among", "all", "players"],
                &["life", "total", "among", "players"],
                &["life", "total", "among", "your", "opponents"],
            ]) {
                return Ok(if op == AggregateOp::Max {
                    ValueExpr::HighestLifeTotal
                } else {
                    player_stat(PlayerRef::LowestLifePlayer, PlayerStatKind::LifeTotal)
                });
            }
            let stat = if ts.eat_phrase(&["mana", "value"]) {
                Some(StatKind::ManaValue)
            } else if ts.eat_any_phrase(&[
                &["power", "and/or", "toughness"],
                &["power", "or", "toughness"],
                &["power"],
            ]) {
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
                && ts.eat_any_word(&["among", "of"])
            {
                if ts.eat_word("them") || ts.eat_phrase(&["the", "milled", "cards"]) {
                    return Ok(any_perm_agg(op, stat));
                }
                let _ = ts.eat_any_word(&["those", "the"]);
                if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                    let acp = ts.checkpoint();
                    if ts.eat_word("and")
                        && let Ok(other) = try_parse(ts, parse_object_filter)
                    {
                        flt = ObjectFilter {
                            predicates: vec![FilterPredicate::AnyOf(vec![flt, other])],
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
    if ts.eat_any_phrase(&[
        &[
            "the",
            "total",
            "power",
            "of",
            "the",
            "sacrificed",
            "creatures",
        ],
        &[
            "the",
            "total",
            "power",
            "of",
            "all",
            "sacrificed",
            "creatures",
        ],
    ]) {
        return Ok(ValueExpr::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(ObjectFilter {
                any_types: CardTypeMask::CREATURE,
                predicates: vec![FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::SacrificedThisWay,
                )],
                ..Default::default()
            }),
        });
    }
    if ts.eat_any_phrase(&[
        &[
            "the",
            "number",
            "of",
            "colors",
            "in",
            "your",
            "commanders'",
            "color",
            "identity",
        ],
        &[
            "the",
            "number",
            "of",
            "colors",
            "in",
            "your",
            "commander's",
            "color",
            "identity",
        ],
    ]) {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Group(
            Box::new(ObjectFilter {
                flags: FilterFlags::IS_COMMANDER,
                owner: Some(ControllerRelation::OwnerYou),
                ..Default::default()
            }),
        ))));
    }
    if ts.eat_phrase(&["the", "amassed", "army's", "power"]) {
        return Ok(obj_stat(
            SubjectRef::Anaphora(AnaphoraRef::ThatCreature),
            StatKind::Power,
        ));
    }
    // `the life <player> lost/gained this turn`
    {
        let lcp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["the", "life"], &["the", "amount", "of", "life"]]) {
            let p_opt = if ts.eat_word("you've") {
                Some(PlayerRef::You)
            } else if ts.eat_word("they've") {
                Some(PlayerRef::ThatPlayer)
            } else {
                try_parse(ts, parse_player_ref).ok()
            };
            if let Some(player) = p_opt {
                if ts.eat_phrase(&["lost", "this", "turn"]) {
                    return Ok(turn_event(TurnEventKind::LifeLost, Some(player)));
                }
                if ts.eat_phrase(&["gained", "this", "turn"]) {
                    return Ok(turn_event(TurnEventKind::LifeGained, Some(player)));
                }
            }
        }
        ts.restore(lcp);
    }
    // `the life total of an opponent with the most life`
    if ts.eat_any_phrase(&[
        &[
            "the", "life", "total", "of", "an", "opponent", "with", "the", "most", "life",
        ],
        &[
            "the", "life", "total", "of", "a", "player", "with", "the", "most", "life",
        ],
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

    if ts.eat_any_phrase(&[&["that", "amount"], &["that", "damage"]]) {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::DealtDamageThisWay,
        ));
    }
    const DEFINITE_STAT_PHRASES: &[(&[&str], AnaphoraRef, StatKind)] = &[
        (
            &["the", "spell's", "mana", "value"],
            AnaphoraRef::ThatSpell,
            StatKind::ManaValue,
        ),
        (
            &["the", "milled", "card's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["the", "discarded", "card's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["the", "exiled", "card's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["the", "creature's", "power"],
            AnaphoraRef::ThatCreature,
            StatKind::Power,
        ),
        (
            &["the", "creature's", "toughness"],
            AnaphoraRef::ThatCreature,
            StatKind::Toughness,
        ),
    ];
    for &(phrase, anaph, stat) in DEFINITE_STAT_PHRASES {
        if ts.eat_phrase(phrase) {
            return Ok(obj_stat(SubjectRef::Anaphora(anaph), stat));
        }
    }
    // `[the total | the] number of ...`
    if ts.eat_any_phrase(&[&["the", "total", "number", "of"], &["the", "number", "of"]]) {
        if ts.eat_phrase(&["creatures", "that", "died", "this", "way"]) {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_types: CardTypeMask::CREATURE,
                predicates: vec![FilterPredicate::AffectedByPriorAction(
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
                    kind: CounterKind::Any,
                });
            }
            ts.restore(acp);
        }
        if ts.eat_any_phrase(&[
            &[
                "cards",
                "in",
                "your",
                "graveyard",
                "that",
                "are",
                "instant",
                "cards",
            ],
            &[
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
            ],
        ]) {
            let _ = ts.eat_comma();
            let _ = ts.eat_word("are");
            if ts.eat_phrase(&["sorcery", "cards"]) {
                let _ = ts.eat_comma();
                if ts.eat_phrase(&["and/or", "have", "an", "adventure"]) {
                    return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                        any_types: CardTypeMask::INSTANT.union(CardTypeMask::SORCERY),
                        flags: FilterFlags::HAS_ADVENTURE,
                        any_card: true,
                        zones: vec![ZoneRef {
                            zone: ZoneKind::Graveyard,
                            owner: ZoneOwner::You,
                        }],
                        ..Default::default()
                    })));
                }
            }
        }
        if ts.eat_any_phrase(&[
            &["permanents", "you've", "sacrificed", "this", "turn"],
            &["permanents", "you", "sacrificed", "this", "turn"],
        ]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::SacrificedThisWay,
            ));
        }
        if ts.eat_any_phrase(&[
            &["tokens", "you", "created", "this", "turn"],
            &["tokens", "you've", "created", "this", "turn"],
            &["treasure", "tokens", "you", "created", "this", "turn"],
            &["treasure", "tokens", "you've", "created", "this", "turn"],
        ]) {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::CreatedThisWay));
        }
        if ts.eat_phrase(&[
            "times", "this", "ability", "has", "resolved", "this", "turn",
        ]) {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        if ts.eat_any_phrase(&[
            &[
                "spells", "you've", "cast", "from", "anywhere", "other", "than", "your", "hand",
                "this", "turn",
            ],
            &[
                "spells", "you", "cast", "from", "anywhere", "other", "than", "your", "hand",
                "this", "turn",
            ],
        ]) {
            return Ok(turn_event(TurnEventKind::SpellCast, Some(PlayerRef::You)));
        }
        {
            let ocp = ts.checkpoint();
            let _ = ts.eat_word("your");
            if ts.eat_any_phrase(&[
                &["opponents", "who"],
                &["players", "who"],
                &["opponents", "that"],
                &["players", "that"],
            ]) {
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
                if ts.eat_any_phrase(&[&["investigated", "this", "way"], &["did", "this", "way"]]) {
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
        if ts.eat_any_phrase(&[
            &["players", "being", "attacked"],
            &["players", "you", "attacked", "this", "combat"],
            &["players", "you", "attacked", "this", "turn"],
        ]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["different", "mana", "values", "among", "cards", "in"])
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Ok(ValueExpr::DistinctManaValues(zref));
        }
        if ts.eat_phrase(&["cards", "in", "all", "players'", "hands"]) {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                any_card: true,
                zones: vec![ZoneRef {
                    zone: ZoneKind::Hand,
                    owner: ZoneOwner::All,
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
                    let _ = ts.eat_any_word(&["library", "hand", "graveyard"])
                        || try_parse(ts, parse_zone_ref).is_ok();
                }
                let _ = ts.eat_phrase(&["this", "turn"]);
                return Ok(ValueExpr::Count(Box::new(ObjectFilter {
                    any_card: true,
                    zones: vec![zref],
                    ..Default::default()
                })));
            }
            ts.restore(gy_cp);
        }
        // Chroma: `the number of <color> mana symbols in the mana costs of <flt>`
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
        if ts.eat_any_phrase(&[
            &["unique", "vowels", "on", "that", "sticker"],
            &["times", "you", "descended", "this", "turn"],
            &["coins", "that", "came", "up", "heads"],
            &["times", "you", "chose", "a", "mode", "for", "that", "spell"],
        ]) || (ts.eat_phrase(&[
            "cards", "you", "removed", "from", "the", "draft", "with", "cards", "named",
        ]) && ts.eat_tilde())
        {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        if ts.eat_any_phrase(&[
            &["opponents", "being", "attacked"],
            &["opponents", "you", "attacked", "this", "turn"],
            &[
                "opponents",
                "who",
                "have",
                "three",
                "or",
                "more",
                "poison",
                "counters",
            ],
            &[
                "your",
                "opponents",
                "with",
                "four",
                "or",
                "more",
                "cards",
                "in",
                "hand",
            ],
        ]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        if ts.eat_phrase(&["unlocked", "doors", "among", "rooms", "you", "control"]) {
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
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
            // `<letter>'s in name stickers on ~`
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
            // `cards you own in exile and in your graveyard that are <flt> or are named ~`
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
                && ts.eat_any_word(&["is", "are", "was", "were"])
            {
                return Ok(ValueExpr::ColorCount(Box::new(subj)));
            }
            ts.restore(c2cp);
        }
        {
            let kcp = ts.checkpoint();
            if ts.eat_word("times") && (ts.eat_tilde() || ts.eat_word("it")) {
                if ts.eat_phrase(&["was", "kicked"]) {
                    return Ok(ValueExpr::TimesKicked);
                }
                if ts.eat_phrase(&["has", "mutated"]) {
                    return Ok(ValueExpr::MutatedCount);
                }
            }
            ts.restore(kcp);
        }
        // `the number of different kinds of counters among <flt>`
        {
            let dcp = ts.checkpoint();
            if ts.eat_phrase(&["different", "kinds", "of", "counters", "among"])
                && let Ok(flt) = try_parse(ts, parse_object_filter)
            {
                return Ok(ValueExpr::DistinctCounterKinds(Box::new(flt)));
            }
            ts.restore(dcp);
        }
        // `the number of <counter_kind> counters among <flt|players>`
        {
            let kcp = ts.checkpoint();
            if let Ok(kind) = try_parse(ts, parse_counter_kind)
                && ts.eat_any_word(&["counters", "counter"])
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
        // `the number of <flt> they've/you've/that player [has] cast this turn`
        {
            let scp = ts.checkpoint();
            if let Ok(_flt) = try_parse(ts, parse_object_filter) {
                const CAST_TURN_PLAYERS: &[(&[&str], PlayerRef)] = &[
                    (&["they've", "cast", "this", "turn"], PlayerRef::ThatPlayer),
                    (&["they", "cast", "this", "turn"], PlayerRef::ThatPlayer),
                    (
                        &["that", "player", "has", "cast", "this", "turn"],
                        PlayerRef::ThatPlayer,
                    ),
                    (
                        &["that", "player", "cast", "this", "turn"],
                        PlayerRef::ThatPlayer,
                    ),
                    (&["you've", "cast", "this", "turn"], PlayerRef::You),
                    (&["you", "cast", "this", "turn"], PlayerRef::You),
                ];
                if let Some(player) = eat_table(ts, CAST_TURN_PLAYERS) {
                    return Ok(turn_event(TurnEventKind::SpellCast, Some(player)));
                }
            }
            ts.restore(scp);
        }
        // `the number of counters on <subj>` / `the number of counters it had on it`
        {
            let bcp = ts.checkpoint();
            if ts.eat_any_word(&["counters", "counter"]) {
                if ts.eat_phrase(&["it", "had", "on", "it"]) {
                    return Ok(ValueExpr::CounterCount {
                        subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
                        kind: CounterKind::Any,
                    });
                }
                if ts.eat_word("on")
                    && let Ok(subject) = try_parse(ts, parse_subject_ref)
                {
                    return Ok(ValueExpr::CounterCount {
                        subject: Box::new(subject),
                        kind: CounterKind::Any,
                    });
                }
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
            return Ok(player_stat(PlayerRef::You, PlayerStatKind::CardsInHand));
        }
        if ts.eat_phrase(&["cards", "looked", "at", "while", "scrying", "this", "way"]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::LookedAtScryingThisWay,
            ));
        }
        if ts.eat_phrase(&["basic", "land", "types", "among", "lands", "you", "control"]) {
            return Ok(ValueExpr::DomainCount(PlayerRef::You));
        }
        if ts.eat_any_phrase(&[
            &["card", "types", "among", "cards", "in"],
            &["permanent", "types", "among", "cards", "in"],
        ]) {
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
                && ts.eat_any_word(&["counters", "counter"])
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
                    && ts.eat_any_word(&["have", "has"])
                {
                    return Ok(ValueExpr::PlayerCounterCount { player, kind });
                }
            }
            ts.restore(ccp);
        }
        let mut flt = parse_object_filter(ts)?;
        if ts.eat_any_phrase(&[
            &[
                "that",
                "entered",
                "the",
                "battlefield",
                "under",
                "your",
                "control",
                "this",
                "turn",
            ],
            &[
                "that", "entered", "under", "your", "control", "this", "turn",
            ],
        ]) {
            flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
            flt.controller = Some(ControllerRelation::You);
        }
        if ts.eat_any_phrase(&[
            &["that", "died", "under", "your", "control", "this", "turn"],
            &["that", "died", "this", "turn"],
        ]) {
            flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
        }
        let _ = ts.eat_any_phrase(&[
            &[
                "controlled",
                "by",
                "the",
                "player",
                "who",
                "controls",
                "the",
                "fewest",
            ],
            &["those", "players", "control"],
            &["that", "player", "controls"],
        ]);
        // Optional trailing `at that time` / `as you cast ~` / `as this ability resolves`
        let _ = ts.eat_any_phrase(&[
            &["at", "that", "time"],
            &["as", "this", "ability", "resolves"],
        ]) || (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde());
        return Ok(ValueExpr::Count(Box::new(flt)));
    }

    // `the [tapped/sacrificed/exiled/...] <noun>'s <power|toughness|mana value>`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["the", "that"]) {
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
                let _ = ts.eat_any_word(&["tapped", "attacking", "blocking", "chosen"]);
                AnaphoraRef::ThatObject
            };
            let _ = ts.eat_any_word(&["creature", "nonland", "instant", "sorcery"]);
            if let Some(noun) = ts.peek_word()
                && noun.ends_with("'s")
            {
                ts.next_word();
                if let Some(stat) = parse_stat_word(ts) {
                    return Ok(obj_stat(SubjectRef::Anaphora(anaph), stat));
                }
            }
        }
        ts.restore(cp);
    }

    // `its/their base power` / `its/their base toughness`
    const BASE_STAT_TABLE: &[(&[&str], StatKind)] = &[
        (&["its", "base", "power"], StatKind::Power),
        (&["their", "base", "power"], StatKind::Power),
        (&["its", "base", "toughness"], StatKind::Toughness),
        (&["their", "base", "toughness"], StatKind::Toughness),
    ];
    if let Some(stat) = eat_table(ts, BASE_STAT_TABLE) {
        return Ok(obj_stat(SubjectRef::Anaphora(AnaphoraRef::It), stat));
    }

    // `~'s power` / `~'s toughness` / `~'s loyalty` / `that creature's power` / `that creature's toughness`
    {
        let cp = ts.checkpoint();
        if ts.eat_tilde() {
            let _ = ts.eat_word("'s");
            if ts.eat_word("power") {
                return Ok(obj_stat(SubjectRef::SelfObject, StatKind::Power));
            }
            if ts.eat_word("toughness") {
                return Ok(obj_stat(SubjectRef::SelfObject, StatKind::Toughness));
            }
            if ts.eat_word("loyalty") {
                return Ok(ValueExpr::CounterCount {
                    subject: Box::new(SubjectRef::SelfObject),
                    kind: CounterKind::Loyalty,
                });
            }
        }
        ts.restore(cp);
    }
    const ANAPHORA_STAT_TABLE: &[(&[&str], AnaphoraRef, StatKind)] = &[
        (
            &["equipped", "creature's", "power"],
            AnaphoraRef::EquippedCreature,
            StatKind::Power,
        ),
        (
            &["enchanted", "creature's", "power"],
            AnaphoraRef::EquippedCreature,
            StatKind::Power,
        ),
        (
            &["equipped", "creature's", "toughness"],
            AnaphoraRef::EquippedCreature,
            StatKind::Toughness,
        ),
        (
            &["enchanted", "creature's", "toughness"],
            AnaphoraRef::EquippedCreature,
            StatKind::Toughness,
        ),
        (
            &["equipped", "creature's", "mana", "value"],
            AnaphoraRef::EquippedCreature,
            StatKind::ManaValue,
        ),
        (
            &["enchanted", "creature's", "mana", "value"],
            AnaphoraRef::EquippedCreature,
            StatKind::ManaValue,
        ),
        (
            &["enchanted", "permanent's", "mana", "value"],
            AnaphoraRef::EquippedCreature,
            StatKind::ManaValue,
        ),
        (
            &["that", "creature's", "power"],
            AnaphoraRef::ThatCreature,
            StatKind::Power,
        ),
        (
            &["that", "creature", "card's", "power"],
            AnaphoraRef::ThatCreature,
            StatKind::Power,
        ),
        (
            &["that", "permanent's", "power"],
            AnaphoraRef::ThatCreature,
            StatKind::Power,
        ),
        (
            &["that", "spell's", "power"],
            AnaphoraRef::ThatCreature,
            StatKind::Power,
        ),
        (
            &["that", "card's", "power"],
            AnaphoraRef::ThatCreature,
            StatKind::Power,
        ),
        (
            &["that", "creature's", "toughness"],
            AnaphoraRef::ThatCreature,
            StatKind::Toughness,
        ),
        (
            &["that", "creature", "card's", "toughness"],
            AnaphoraRef::ThatCreature,
            StatKind::Toughness,
        ),
        (
            &["that", "permanent's", "toughness"],
            AnaphoraRef::ThatCreature,
            StatKind::Toughness,
        ),
        (
            &["that", "spell's", "toughness"],
            AnaphoraRef::ThatCreature,
            StatKind::Toughness,
        ),
        (
            &["that", "card's", "toughness"],
            AnaphoraRef::ThatCreature,
            StatKind::Toughness,
        ),
        (
            &["that", "card's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "nonland", "card's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "creature", "card's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "spell's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "creature's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "permanent's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "enchantment's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "artifact's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "planeswalker's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
        (
            &["that", "saga's", "mana", "value"],
            AnaphoraRef::ThatCard,
            StatKind::ManaValue,
        ),
    ];
    for &(phrase, anaph, stat) in ANAPHORA_STAT_TABLE {
        if ts.eat_phrase(phrase) {
            return Ok(obj_stat(SubjectRef::Anaphora(anaph), stat));
        }
    }
    // `target <noun>'s mana value/power/toughness`
    {
        let tcp = ts.checkpoint();
        if ts.eat_word("target") {
            let _ = ts.eat_word("nonland");
            if ts.eat_any_word(&[
                "permanent's",
                "creature's",
                "spell's",
                "artifact's",
                "enchantment's",
                "card's",
            ]) && let Some(stat) = parse_stat_word(ts)
            {
                return Ok(obj_stat(
                    SubjectRef::TargetObject {
                        filter: Box::new(ObjectFilter {
                            any_permanent: true,
                            ..Default::default()
                        }),
                        count: TargetCount::Exact(1),
                        another: false,
                    },
                    stat,
                ));
            }
        }
        ts.restore(tcp);
    }

    // `its mana value` / `its power` / `its toughness` / `its loyalty` / `his/her power/toughness`
    if ts.eat_phrase(&["its", "loyalty"]) {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(SubjectRef::Anaphora(AnaphoraRef::It)),
            kind: CounterKind::Loyalty,
        });
    }
    const PRONOUN_STAT_TABLE: &[(&[&str], StatKind)] = &[
        (&["its", "mana", "value"], StatKind::ManaValue),
        (&["its", "power"], StatKind::Power),
        (&["his", "power"], StatKind::Power),
        (&["her", "power"], StatKind::Power),
        (&["its", "toughness"], StatKind::Toughness),
        (&["his", "toughness"], StatKind::Toughness),
        (&["her", "toughness"], StatKind::Toughness),
        (&["their", "toughness"], StatKind::Toughness),
        (&["their", "power"], StatKind::Power),
    ];
    if let Some(stat) = eat_table(ts, PRONOUN_STAT_TABLE) {
        return Ok(obj_stat(SubjectRef::Anaphora(AnaphoraRef::It), stat));
    }

    // `that many` / `that much`
    if ts.eat_any_phrase(&[&["that", "many"], &["that", "much"]]) {
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
