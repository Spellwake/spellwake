//! Compositional `for each ...` / `for every ...` [`ValueExpr`] parser for `v2`.

use crate::ast::{PlayerStatKind, Rounding};
use crate::v2::filter::{
    parse_counter_kind, parse_object_filter, parse_player_ref, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, CounterKind, ManaSymbol, ObjectFilter, PlayerRef, PriorActionKind, SubjectRef,
    TurnEventKind, ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};

use super::{eat_table, turn_event};

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

    // `time you've cast your/a commander / it's been cast from the command zone this game`
    if (ts.eat_phrase(&["time", "you've", "cast"])
        && (ts.eat_word("your") || ts.eat_word("a"))
        && ts.eat_phrase(&[
            "commander",
            "from",
            "the",
            "command",
            "zone",
            "this",
            "game",
        ]))
        || ts.eat_phrase(&[
            "time", "it's", "been", "cast", "from", "the", "command", "zone", "this", "game",
        ])
    {
        return Ok(ValueExpr::CommanderCastCount(PlayerRef::You));
    }
    if ts.eat_phrase(&["mana", "spent", "to", "cast"]) && (ts.eat_tilde() || ts.eat_word("it")) {
        return Ok(ValueExpr::ManaPaidThisWay);
    }
    if ts.eat_phrase(&["different", "mana", "value", "among", "cards", "in"])
        && let Ok(zref) = try_parse(ts, parse_zone_ref)
    {
        return Ok(ValueExpr::DistinctManaValues(zref));
    }
    {
        let dcp = ts.checkpoint();
        let _ = ts.eat_word("different");
        if ts.eat_phrase(&["card", "type", "among"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(ValueExpr::DistinctCardTypesAmong(Box::new(flt)));
        }
        ts.restore(dcp);
    }
    {
        let ccp = ts.checkpoint();
        if ts.eat_phrase(&["color", "of"])
            && let Ok(subj) = try_parse(ts, parse_subject_ref)
        {
            return Ok(ValueExpr::ColorCount(Box::new(subj)));
        }
        ts.restore(ccp);
    }
    if ts.eat_phrase(&["counter", "on"])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(subject),
            kind: CounterKind::Any,
        });
    }

    // `target beyond the first` / `color of mana spent to cast ~` / `opponent you have` / `of its colors` / `card in your hand`
    if ts.eat_phrase(&["target", "beyond", "the", "first"]) {
        return Ok(ValueExpr::TargetsBeyondFirst);
    }
    if ts.eat_phrase(&["color", "of", "mana", "spent", "to", "cast"])
        && (ts.eat_tilde() || ts.eat_word("it") || ts.eat_phrase(&["that", "spell"]))
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
    if matches!(
        ts.peek().map(|t| &t.kind),
        Some(TokenKind::Mana(ManaSymbol::Energy))
    ) {
        let ecp = ts.checkpoint();
        ts.next_tok();
        if ts.eat_phrase(&["paid", "this", "way"]) {
            return Ok(ValueExpr::PriorActionCount(
                PriorActionKind::EnergyPaidThisWay,
            ));
        }
        ts.restore(ecp);
    }
    if ts.eat_any_phrase(&[
        &["of", "the", "exiled", "cards"],
        &["card", "exiled", "this", "way"],
    ]) {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ExiledThisWay));
    }
    if ts.eat_phrase(&["counter", "removed", "this", "way"]) {
        return Ok(ValueExpr::PriorActionCount(
            PriorActionKind::CountersRemovedThisWay,
        ));
    }
    if ts.eat_phrase(&[
        "creature", "that", "died", "under", "your", "control", "this", "turn",
    ]) {
        return Ok(turn_event(
            TurnEventKind::CreatureDied,
            Some(PlayerRef::You),
        ));
    }
    if ts.eat_phrase(&["creature", "that", "died", "this", "turn"]) {
        return Ok(turn_event(TurnEventKind::CreatureDied, None));
    }
    {
        let onecp = ts.checkpoint();
        if ts.next_number(false) == Some(1) {
            if ts.eat_phrase(&["damage", "prevented", "this", "way"]) {
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DamagePreventedThisWay,
                ));
            }
            if ts.eat_phrase(&["damage", "dealt", "this", "way"]) {
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
                ));
            }
            if ts.eat_phrase(&["life", "you", "gained", "this", "turn"]) {
                return Ok(turn_event(TurnEventKind::LifeGained, Some(PlayerRef::You)));
            }
            if ts.eat_any_phrase(&[
                &["life", "they", "gained", "this", "turn"],
                &["life", "that", "player", "gained", "this", "turn"],
            ]) {
                return Ok(turn_event(
                    TurnEventKind::LifeGained,
                    Some(PlayerRef::ThatPlayer),
                ));
            }
            if ts.eat_phrase(&["life", "you", "lost", "this", "turn"]) {
                return Ok(turn_event(TurnEventKind::LifeLost, Some(PlayerRef::You)));
            }
            if ts.eat_any_phrase(&[
                &["life", "your", "opponents", "have", "lost", "this", "turn"],
                &["life", "your", "opponents", "lost", "this", "turn"],
            ]) {
                return Ok(turn_event(
                    TurnEventKind::LifeLost,
                    Some(PlayerRef::EachOpponent),
                ));
            }
        }
        ts.restore(onecp);
    }
    {
        let pcp = ts.checkpoint();
        let opp_only =
            if ts.eat_any_phrase(&[&["opponent", "who"], &["of", "your", "opponents", "who"]]) {
                Some(true)
            } else if ts.eat_phrase(&["player", "who"]) {
                Some(false)
            } else {
                None
            };
        if let Some(opponents_only) = opp_only {
            const PLAYER_WHO_EVENTS: &[(&[&str], TurnEventKind)] = &[
                (&["lost", "life", "this", "turn"], TurnEventKind::LifeLost),
                (
                    &["discarded", "a", "card", "this", "turn"],
                    TurnEventKind::CardDiscarded,
                ),
                (
                    &["discarded", "one", "or", "more", "cards", "this", "turn"],
                    TurnEventKind::CardDiscarded,
                ),
                (
                    &["drew", "a", "card", "this", "turn"],
                    TurnEventKind::CardDrawn,
                ),
            ];
            if let Some(event) = eat_table(ts, PLAYER_WHO_EVENTS) {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event,
                    opponents_only,
                });
            }
        }
        ts.restore(pcp);
    }
    if ts.eat_phrase(&["kind", "of", "counter", "among"])
        && let Ok(flt) = try_parse(ts, parse_object_filter)
    {
        return Ok(ValueExpr::DistinctCounterKinds(Box::new(flt)));
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
    const COLOR_ANAPHORA: &[(&[&str], AnaphoraRef)] = &[
        (&["of", "its", "colors"], AnaphoraRef::It),
        (&["of", "that", "spell's", "colors"], AnaphoraRef::ThatSpell),
        (
            &["of", "the", "sacrificed", "creature's", "colors"],
            AnaphoraRef::SacrificedObject,
        ),
        (
            &["of", "the", "sacrificed", "permanent's", "colors"],
            AnaphoraRef::SacrificedObject,
        ),
        (
            &["of", "that", "creature's", "colors"],
            AnaphoraRef::ThatObject,
        ),
        (
            &["of", "that", "permanent's", "colors"],
            AnaphoraRef::ThatObject,
        ),
        (
            &["of", "the", "exiled", "card's", "colors"],
            AnaphoraRef::ThatObject,
        ),
        (
            &["of", "the", "discarded", "card's", "colors"],
            AnaphoraRef::ThatObject,
        ),
    ];
    if let Some(anaph) = eat_table(ts, COLOR_ANAPHORA) {
        return Ok(ValueExpr::ColorCount(Box::new(SubjectRef::Anaphora(anaph))));
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
            return Ok(ValueExpr::Count(Box::new(ObjectFilter {
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
            && ts.eat_any_word(&["counter", "counters"])
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
                && ts.eat_any_word(&["have", "has"])
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
