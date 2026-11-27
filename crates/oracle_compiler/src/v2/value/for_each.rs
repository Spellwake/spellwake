//! Compositional or each ... / or every ... [ValueExpr] parser for 2.

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
    {
        let mcp = ts.checkpoint();
        if ts.eat_mana_cost().is_some() {
            if ts.eat_word("or") {
                let _ = ts.eat_mana_cost();
            }
            if (ts.eat_phrase(&["spent", "to", "cast"]) && (ts.eat_tilde() || ts.eat_word("it")))
                || ts.eat_phrase(&["spent", "this", "way"])
            {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
        }
        ts.restore(mcp);
    }
    if ts.eat_phrase(&["different", "mana", "value", "among", "cards", "in"])
        && let Ok(zref) = try_parse(ts, parse_zone_ref)
    {
        return Ok(ValueExpr::DistinctManaValues(zref));
    }
    {
        let dcp = ts.checkpoint();
        let _ = ts.eat_word("different");
        if ts.eat_any_phrase(&[
            &["mana", "value"],
            &["mana", "cost"],
            &["power"],
            &["toughness"],
            &["card", "type"],
            &["creature", "type"],
        ]) && ts.eat_word("among")
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
            let _ = ts.eat_phrase(&["it", "targets"]);
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
    if ts.eat_phrase(&["counter", "among", "players", "and", "permanents"]) {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(SubjectRef::All(Box::new(ObjectFilter {
                any_permanent: true,
                ..Default::default()
            }))),
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
    if ts.eat_any_phrase(&[
        &["opponent", "you", "have"],
        &["opponent", "you're", "attacking"],
        &["opponent", "you", "attacked"],
        &["player", "being", "attacked"],
    ]) {
        return Ok(ValueExpr::OpponentsCount);
    }
    if ts.eat_any_phrase(&[
        &["opponent", "dealt", "damage"],
        &["opponent", "dealt", "combat", "damage"],
    ]) {
        let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "way"]]);
        return Ok(ValueExpr::PlayersWhoDidThisTurn {
            event: TurnEventKind::LifeLost,
            opponents_only: true,
        });
    }
    if matches!(
        ts.peek().map(|t| &t.kind),
        Some(TokenKind::Mana(ManaSymbol::Energy))
    ) {
        let ecp = ts.checkpoint();
        ts.next_tok();
        if ts.eat_any_phrase(&[
            &["paid", "this", "way"],
            &["you've", "paid", "or", "lost", "this", "turn"],
        ]) {
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
    const FOR_EACH_THAT_MANY: &[&[&str]] = &[
        &["card", "manifested", "this", "way"],
        &["other", "result"],
        &["flip", "you", "win"],
        &["flip", "you", "won"],
        &["flip"],
        &["time", "it", "regenerated", "this", "turn"],
        &["of", "those", "creatures"],
        &["of", "those", "permanents"],
        &["of", "those", "cards"],
        &["creature", "chosen", "this", "way"],
        &["unique", "vowel", "on", "that", "sticker"],
        &["counter", "removed", "this", "way"],
        &["counter", "removed"],
        &["coin", "that", "comes", "up", "heads"],
        &["even", "result"],
        &["odd", "result"],
        &["different", "result"],
        &["one", "they", "gained", "control", "of", "this", "way"],
        &["card", "you", "put", "on", "top", "of", "your", "library"],
        &["time", "it", "has", "attacked", "this", "turn"],
        &["of", "the", "chosen", "colors", "it", "is"],
        &[
            "graveyard",
            "with",
            "seven",
            "or",
            "more",
            "cards",
            "in",
            "it",
        ],
        &[
            "+1/+1",
            "counter",
            "you've",
            "put",
            "on",
            "creatures",
            "under",
            "your",
            "control",
            "this",
            "turn",
        ],
    ];
    if ts.eat_any_phrase(FOR_EACH_THAT_MANY) {
        return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
    }
    if ts.eat_any_phrase(&[
        &[
            "creature", "that", "died", "under", "your", "control", "this", "turn",
        ],
        &["creature", "that", "died", "this", "turn"],
    ]) {
        return Ok(turn_event(
            TurnEventKind::CreatureDied,
            Some(PlayerRef::You),
        ));
    }
    {
        let lcp = ts.checkpoint();
        if ts.eat_phrase(&["card", "less", "than"])
            && ts.next_number(false).is_some()
            && ts.eat_phrase(&["a", "player", "draws", "this", "way"])
        {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        ts.restore(lcp);
        if ts.eat_phrase(&[
            "colored", "mana", "symbol", "in", "the", "milled", "card's", "mana", "cost",
        ]) {
            return Ok(ValueExpr::ColorsOfManaSpent);
        }
        if let Some(w) = ts.peek_word()
            && let Some(c) = crate::vocab::lookup_color(&w.to_ascii_lowercase())
        {
            ts.next_word();
            if ts.eat_phrase(&["mana", "symbol", "in", "the", "mana", "costs", "of"])
                && try_parse(ts, parse_object_filter).is_ok()
            {
                return Ok(ValueExpr::Devotion(c));
            }
        }
        ts.restore(lcp);
    }
    // `for each target` (`Hinata, Dawn-Crowned`) / `for each player` (`Benediction of Moons`)
    if ts.eat_word("target") {
        return Ok(ValueExpr::TargetsBeyondFirst);
    }
    {
        let pcp = ts.checkpoint();
        if ts.eat_word("player")
            && !ts.peek_word().is_some_and(|w| {
                w.eq_ignore_ascii_case("who")
                    || w.eq_ignore_ascii_case("being")
                    || w.eq_ignore_ascii_case("controls")
            })
        {
            return Ok(ValueExpr::OpponentsCount);
        }
        ts.restore(pcp);
    }
    // `for each unspent [<color>] mana you have` (`Omnath, Locus of Mana`, `Omnath, Locus of the Void`)
    {
        let ucp = ts.checkpoint();
        if ts.eat_word("unspent") {
            if let Some(w) = ts.peek_word()
                && crate::vocab::lookup_color(&w.to_ascii_lowercase()).is_some()
            {
                ts.next_word();
            }
            if ts.eat_phrase(&["mana", "you", "have"]) {
                return Ok(ValueExpr::ManaPaidThisWay);
            }
        }
        ts.restore(ucp);
    }
    // `for each name sticker on (it|~) [with <N> or (more|fewer) letters]`
    {
        let scp = ts.checkpoint();
        if ts.eat_phrase(&["name", "sticker", "on"]) && (ts.eat_word("it") || ts.eat_tilde()) {
            let wcp = ts.checkpoint();
            if ts.eat_word("with")
                && ts.next_number(false).is_some()
                && (ts.eat_phrase(&["or", "more", "letters"])
                    || ts.eat_phrase(&["or", "fewer", "letters"]))
            {
                // Consumed letter-count qualifier.
            } else {
                ts.restore(wcp);
            }
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        ts.restore(scp);
    }
    // `for each supertype, card type, and subtype it has` / `for each of its creature types [, to a maximum of <N>]`
    // / `for each point of <kw> it has` / `for each <word> vote`
    if ts.eat_word("supertype")
        && ts.eat_comma()
        && ts.eat_phrase(&["card", "type"])
        && ts.eat_comma()
        && ts.eat_phrase(&["and", "subtype", "it", "has"])
    {
        return Ok(ValueExpr::DistinctCardTypesAmong(Box::default()));
    }
    if ts.eat_phrase(&["of", "its", "creature", "types"]) {
        let m_cp = ts.checkpoint();
        if !(ts.eat_comma()
            && ts.eat_phrase(&["to", "a", "maximum", "of"])
            && ts.next_number(false).is_some())
        {
            ts.restore(m_cp);
        }
        return Ok(ValueExpr::DistinctCardTypesAmong(Box::default()));
    }
    {
        let pcp = ts.checkpoint();
        if ts.eat_phrase(&["point", "of"])
            && try_parse(ts, crate::v2::filter::parse_keyword_kind).is_ok()
            && ts.eat_phrase(&["it", "has"])
        {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        ts.restore(pcp);
        // `for each <choice> vote` (`Capital Punishment`, `Selvala's Stampede`, `Fateful Tempest`, `Truth or Consequences`)
        if ts.next_word().is_some() && ts.eat_word("vote") {
            return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
        ts.restore(pcp);
        // `for each opponent whose life total is less than half their starting life total` (`Anya, Merciless Angel`)
        if ts.eat_phrase(&[
            "opponent", "whose", "life", "total", "is", "less", "than", "half", "their",
            "starting", "life", "total",
        ]) {
            return Ok(ValueExpr::OpponentsCount);
        }
        ts.restore(pcp);
    }
    if ts.eat_any_phrase(&[
        &["card", "drawn", "this", "way"],
        &["cards", "drawn", "this", "way"],
    ]) {
        return Ok(turn_event(TurnEventKind::CardDrawn, Some(PlayerRef::You)));
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
                let _ = ts.eat_any_phrase(&[&["this", "turn"], &["this", "way"]]);
                return Ok(ValueExpr::PriorActionCount(
                    PriorActionKind::DealtDamageThisWay,
                ));
            }
            if ts.eat_any_phrase(&[
                &["life", "you", "gained", "this", "turn"],
                &["life", "you", "gained"],
            ]) {
                return Ok(turn_event(TurnEventKind::LifeGained, Some(PlayerRef::You)));
            }
            if ts.eat_any_phrase(&[
                &["life", "they", "gained", "this", "turn"],
                &["life", "they", "gained"],
                &["life", "that", "player", "gained"],
            ]) {
                return Ok(turn_event(
                    TurnEventKind::LifeGained,
                    Some(PlayerRef::ThatPlayer),
                ));
            }
            if ts.eat_any_phrase(&[
                &["life", "they", "paid", "this", "way"],
                &["life", "you", "paid", "this", "way"],
                &["life", "paid", "this", "way"],
            ]) {
                return Ok(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
            }
            if ts.eat_any_phrase(&[
                &["life", "you", "lost", "this", "turn"],
                &["life", "you", "lost"],
                &["life", "lost", "this", "way"],
                &["life", "your", "opponents", "have", "lost", "this", "turn"],
                &["life", "your", "opponents", "lost", "this", "turn"],
            ]) {
                return Ok(turn_event(TurnEventKind::LifeLost, Some(PlayerRef::You)));
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
                    &["drew", "two", "or", "more", "cards", "this", "turn"],
                    TurnEventKind::CardDrawn,
                ),
                (
                    &["drew", "a", "card", "this", "turn"],
                    TurnEventKind::CardDrawn,
                ),
                (
                    &["drew", "a", "card", "this", "way"],
                    TurnEventKind::CardDrawn,
                ),
                (&["drew", "cards", "this", "way"], TurnEventKind::CardDrawn),
            ];
            if let Some(event) = eat_table(ts, PLAYER_WHO_EVENTS) {
                return Ok(ValueExpr::PlayersWhoDidThisTurn {
                    event,
                    opponents_only,
                });
            }
            if ts.eat_phrase(&["controls", "fewer"])
                && try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_phrase(&["than", "you"])
            {
                return Ok(ValueExpr::OpponentsCount);
            }
            if ts.eat_any_phrase(&[
                &["has", "one", "or", "fewer", "cards", "in", "hand"],
                &["has", "more", "cards", "in", "their", "hand", "than", "you"],
                &["controls", "one", "or", "more", "of", "those", "permanents"],
                &["has", "lost", "the", "game"],
            ]) {
                return Ok(ValueExpr::OpponentsCount);
            }
            const PLAYER_WHO_EVENTS_TAIL: &[(&[&str], TurnEventKind)] = &[
                (
                    &["was", "dealt", "damage", "this", "turn"],
                    TurnEventKind::LifeLost,
                ),
                (
                    &["was", "dealt", "combat", "damage", "this", "turn"],
                    TurnEventKind::LifeLost,
                ),
                (
                    &[
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
                    ],
                    TurnEventKind::SpellCast,
                ),
                (
                    &[
                        "had", "two", "or", "more", "lands", "enter", "under", "their", "control",
                        "this", "turn",
                    ],
                    TurnEventKind::SpellCast,
                ),
            ];
            if let Some(event) = eat_table(ts, PLAYER_WHO_EVENTS_TAIL) {
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
    if ts.eat_phrase(&["kind", "of", "counter", "on"])
        && let Ok(subject) = try_parse(ts, parse_subject_ref)
    {
        return Ok(ValueExpr::CounterCount {
            subject: Box::new(subject),
            kind: CounterKind::Any,
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
            if ts.eat_any_phrase(&[
                &["removed", "this", "way"],
                &["put", "on", "it", "this", "way"],
            ]) {
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
