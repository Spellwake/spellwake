//! Numeric value expression (`ValueExprAst`) and `for each` scaling parsers.

use super::object::parse_object_filter;
use super::subject::{parse_player_ref, parse_subject_or_target};
use crate::ast::{
    AggregateOp, CardTypeMask, FilterPredicateAst, PlayerRefAst, PlayerStatKind,
    PriorActionRefKind, Rounding, StatKind, SubjectRefAst, ValueExprAst,
};
use crate::combinators::{
    anaphora, any_card_flt, box_anaphora, grp, obj_type, split_once_any, strip_prefix_any,
    strip_suffix_any, trim_punct, val_div, val_obj_stat, val_player_stat, vconst,
};
use crate::preprocess::is_physical_tabletop_clause;
use crate::vocab::{Vocabulary, parse_number_word};

pub fn parse_value_expr(phrase: &str, vocab: &Vocabulary) -> Result<ValueExprAst, String> {
    let raw = trim_punct(phrase);
    if raw.eq_ignore_ascii_case("the number they guessed") {
        return Ok(ValueExprAst::PriorResult);
    }
    if raw.is_empty() || raw.contains('\u{2014}') {
        return Err("empty_value".to_string());
    }
    let low_full = raw.to_ascii_lowercase();
    let low = strip_suffix_any(
        &low_full,
        &[
            " as this ability resolves",
            " as this spell resolves",
            " as you cast ~",
            " as you cast this spell",
        ],
    )
    .unwrap_or(&low_full)
    .trim()
    .to_string();

    if low == "x" {
        return Ok(ValueExprAst::ChosenX);
    }
    // Wave E6.2: Max / Min ("<e1> or <e2>, whichever is greater/smaller")
    if let Some(before_wg) = low.strip_suffix(", whichever is greater")
        && let Some((lhs_raw, rhs_raw)) = before_wg.split_once(" or ")
        && let (Ok(lhs), Ok(rhs)) = (
            parse_value_expr(lhs_raw.trim(), vocab),
            parse_value_expr(rhs_raw.trim(), vocab),
        )
    {
        return Ok(ValueExprAst::Max(Box::new(lhs), Box::new(rhs)));
    }
    // Compositional "<expr1> in excess of <expr2>" -> Sub(e1, e2)
    if let Some((lhs_raw, rhs_raw)) = low.split_once(" in excess of ")
        && let (Ok(lhs), Ok(rhs)) = (
            parse_value_expr(lhs_raw.trim(), vocab),
            parse_value_expr(rhs_raw.trim(), vocab),
        )
    {
        return Ok(ValueExprAst::Sub(Box::new(lhs), Box::new(rhs)));
    }
    // Compositional commander mana value:
    // "the [greatest ]mana value of a commander <who> own[s] on the battlefield or in the command zone"
    if let Some(after) = low.strip_prefix("the ")
        && let Some(tail) = after.strip_suffix(" on the battlefield or in the command zone")
    {
        let (greatest, rest) = match tail.strip_prefix("greatest ") {
            Some(r) => (true, r),
            None => (false, tail),
        };
        if let Some(who_raw) = rest
            .strip_prefix("mana value of a commander ")
            .and_then(|r| r.strip_suffix(" owns").or_else(|| r.strip_suffix(" own")))
            && let Some(owner) = parse_player_ref(who_raw.trim())
        {
            return Ok(ValueExprAst::CommanderManaValue { owner, greatest });
        }
    }
    // Compositional "the number of cards <who>['ve| have| has] drawn this turn"
    if let Some(after) = low.strip_prefix("the number of cards ")
        && let Some(who_part) = after.strip_suffix(" drawn this turn")
    {
        let who_clean = strip_suffix_any(&who_part, &["'ve", " have", " has"])
            .unwrap_or(who_part)
            .trim();
        if let Some(player) = parse_player_ref(who_clean) {
            return Ok(ValueExprAst::EventCountThisTurn {
                event: "cards_drawn_this_turn".to_string(),
                player: Some(player),
            });
        }
    }
    // Wave E6.2: Targeted semantic value expressions
    match low.as_str() {
        "the number of times it's been cast from the command zone this game" => {
            return Ok(ValueExprAst::TimesCastFromCommandZone(Box::new(anaphora(
                "it",
            ))));
        }
        "the number of cards they drew this way" => {
            return Ok(ValueExprAst::CountDrawnThisWay(PlayerRefAst::ThatPlayer));
        }
        "the amount of {e} paid this way" => {
            return Ok(ValueExprAst::EnergyPaidThisWay);
        }
        "mana from treasures spent to activate this ability" => {
            return Ok(ValueExprAst::ManaSpentToActivate {
                mana_filter: Some("from_treasure".to_string()),
            });
        }
        "mana from a treasure that was spent to cast ~" => {
            return Ok(ValueExprAst::ManaSpentToCast {
                mana_filter: Some("from_treasure".to_string()),
            });
        }
        "the difference" | "that amount of excess damage" | "the excess" => {
            return Ok(ValueExprAst::PriorResult);
        }
        "the damage prevented this way" | "the amount of damage prevented this way" => {
            return Ok(ValueExprAst::DamagePreventedThisWay);
        }
        "the number of counters removed this way" | "counters removed this way" => {
            return Ok(ValueExprAst::CountersRemovedThisWay);
        }
        "the number of cards exiled this way" => {
            return Ok(ValueExprAst::CardsExiledThisWay);
        }
        "spell you've cast this turn other than the first" => {
            return Ok(ValueExprAst::Max(
                vconst(0),
                Box::new(ValueExprAst::Sub(
                    Box::new(ValueExprAst::EventCountThisTurn {
                        event: "spells_cast_this_turn".to_string(),
                        player: Some(PlayerRefAst::You),
                    }),
                    vconst(1),
                )),
            ));
        }
        _ => {}
    }
    // Compositional Wave E8 numeric expressions
    if let Some(after_dmg) = strip_prefix_any(
        &low,
        &["the damage already dealt to ", "damage already dealt to "],
    ) && let Some(subj_raw) = after_dmg.strip_suffix(" this turn")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::DamageDealtToSubjectThisTurn(Box::new(subj)));
    }
    if low == "its total toxic value" {
        return Ok(ValueExprAst::TotalToxicValue(Box::new(anaphora("it"))));
    }
    if let Some(after_un) = low.strip_prefix("the amount of unspent mana ")
        && let Some(who_raw) = strip_suffix_any(&after_un, &[" have", " has"])
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(ValueExprAst::UnspentManaAmount(player));
    }
    if let Some(after_num) = low.strip_prefix("the number of ")
        && let Some((letter_raw, subj_raw)) = after_num.split_once("'s in name stickers on ")
        && letter_raw.len() == 1
        && let Some(letter) = letter_raw.chars().next()
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::StickerLetterCount {
            subject: Box::new(subj),
            letter,
        });
    }
    // Cluster 24: Exponentiation (`2ˣ`, `2^x`, `2²`, etc.)
    if let Some(base_str) = low
        .strip_suffix('\u{02e3}')
        .or_else(|| low.strip_suffix("^x"))
        && let Ok(base_n) = base_str.trim().parse::<i32>()
    {
        return Ok(ValueExprAst::Pow {
            base: vconst(base_n),
            exp: Box::new(ValueExprAst::ChosenX),
        });
    }
    // Cluster 24: Generalized `half [of] <expr>, rounded (up|down)`
    if let Some(after_half) = strip_prefix_any(&low, &["half of ", "half "]) {
        for (suf, rounding) in [
            (", rounded up", Rounding::Up),
            (", rounded down", Rounding::Down),
            (" rounded up", Rounding::Up),
            (" rounded down", Rounding::Down),
        ] {
            if let Some(inner_raw) = after_half.strip_suffix(suf)
                && let Ok(inner) = parse_value_expr(inner_raw.trim(), vocab)
            {
                return Ok(val_div(inner, 2, rounding));
            }
        }
    }
    // Cluster 14: Craft LKI aggregate stats (`the total power/toughness/mana value of the exiled cards used to craft it|~`)
    for (pat, stat) in [
        (
            "the total power of the exiled cards used to craft ",
            StatKind::Power,
        ),
        (
            "the total toughness of the exiled cards used to craft ",
            StatKind::Toughness,
        ),
        (
            "the total mana value of the exiled cards used to craft ",
            StatKind::ManaValue,
        ),
    ] {
        if let Some(tail) = low.strip_prefix(pat)
            && matches!(tail.trim(), "it" | "~")
        {
            return Ok(val_obj_stat(SubjectRefAst::CraftExiledCards, stat));
        }
    }
    if let Some(rest) = low.strip_prefix("twice ")
        && let Ok(inner) = parse_value_expr(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::Mul(vconst(2), Box::new(inner)));
    }
    if let Some((mult_raw, rest)) = low.split_once(" times ")
        && let Some(mult) = parse_number_word(mult_raw.trim())
        && let Ok(inner) = parse_value_expr(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::Mul(vconst(mult), Box::new(inner)));
    }
    if low == "its base power" {
        return Ok(ValueExprAst::ObjectStat {
            subject: box_anaphora("it"),
            stat: StatKind::Power,
        });
    }
    // Strip trailing evaluation-time qualifiers (" as you activate this ability", " at that time")
    for suf in [" as you activate this ability", " at that time"] {
        if let Some(trimmed) = low.strip_suffix(suf)
            && let Ok(v) = parse_value_expr(trimmed.trim(), vocab)
        {
            return Ok(v);
        }
    }
    // Pure anaphoric prior-action references (must precede generic rules only when truly anaphoric)
    if matches!(
        low.as_str(),
        "that much"
            | "that many"
            | "that amount"
            | "that amount of"
            | "that damage"
            | "that excess damage"
            | "that number"
            | "the last chosen number"
            | "the highest number"
            | "the other result"
            | "the difference"
            | "the result"
            | "that result"
            | "the excess"
            | "the total of those results"
            | "the damage dealt this way"
    ) || (low.starts_with("the ")
        && low.contains("excess damage dealt")
        && low.ends_with(" this way"))
    {
        return Ok(ValueExprAst::PriorResult);
    }
    // Compositional life lost/gained this turn for any player (longest auxiliary suffixes first!)
    if let Some(rest) = strip_prefix_any(
        &low,
        &[
            "the total amount of life ",
            "the total life ",
            "the amount of life ",
            "the life ",
        ],
    ) {
        if let Some(who_raw) = strip_suffix_any(
            &rest,
            &[
                " have lost this turn",
                " has lost this turn",
                "'ve lost this turn",
                " lost this turn",
            ],
        ) {
            let clean = who_raw.strip_prefix("lost by ").unwrap_or(who_raw).trim();
            if let Some(player) = parse_player_ref(clean) {
                return Ok(ValueExprAst::LifeChangedThisTurn {
                    player,
                    gained: false,
                });
            }
        }
        if let Some(who_raw) = rest.strip_prefix("lost by ")
            && let Some(p_raw) = who_raw.strip_suffix(" this turn")
            && let Some(player) = parse_player_ref(p_raw.trim())
        {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player,
                gained: false,
            });
        }
        if let Some(who_raw) = strip_suffix_any(
            &rest,
            &[
                " have gained this turn",
                " has gained this turn",
                "'ve gained this turn",
                " gained this turn",
                " gained",
            ],
        ) && let Some(player) = parse_player_ref(who_raw.trim())
        {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player,
                gained: true,
            });
        }
    }
    if low == "their total power" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Power,
            filter: Box::new(any_card_flt()),
        });
    }
    if low == "their total toughness" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::Toughness,
            filter: Box::new(any_card_flt()),
        });
    }
    if low == "their total mana value" {
        return Ok(ValueExprAst::AggregateStat {
            op: AggregateOp::Sum,
            stat: StatKind::ManaValue,
            filter: Box::new(any_card_flt()),
        });
    }
    if let Some(n) = parse_number_word(&low) {
        return Ok(ValueExprAst::Const(n));
    }
    if let Some(rest) = low.strip_prefix("equal to ") {
        return parse_value_expr(rest, vocab);
    }

    if let Some((l, r)) = low.split_once(" plus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Add(Box::new(lv), Box::new(rv)));
    }
    if let Some((l, r)) = low.split_once(" minus ")
        && let (Ok(lv), Ok(rv)) = (parse_value_expr(l, vocab), parse_value_expr(r, vocab))
    {
        return Ok(ValueExprAst::Sub(Box::new(lv), Box::new(rv)));
    }

    // "your devotion to <colors>" & pre-Theros devotion phrasing
    if let Some(colors) = low.strip_prefix("your devotion to ") {
        return Ok(ValueExprAst::Devotion {
            colors: colors.trim().to_string(),
        });
    }
    if let Some(after) = low.strip_prefix("the number of ")
        && let Some(colors) = after
            .strip_suffix(" mana symbols in the mana costs of permanents you control")
            .or_else(|| {
                after.strip_suffix(" mana symbols in the mana costs of creatures you control")
            })
    {
        return Ok(ValueExprAst::Devotion {
            colors: colors.trim().to_string(),
        });
    }
    if low == "the total number of cards in all players' hands" {
        return Ok(ValueExprAst::TotalCardsInAllHands);
    }
    if low == "the number of creatures in your party" {
        return Ok(ValueExprAst::PartySize);
    }
    if let Some(target) = low.strip_prefix("the amount of mana spent to cast ")
        && matches!(
            target.trim(),
            "~" | "it" | "her" | "him" | "this spell" | "that spell"
        )
    {
        return Ok(ValueExprAst::ManaSpentToCast { mana_filter: None });
    }
    if let Some(after) = low.strip_prefix("the amount of ")
        && let Some(sym) = after
            .strip_suffix(" spent to cast ~")
            .or_else(|| after.strip_suffix(" spent to cast it"))
            .or_else(|| after.strip_suffix(" spent to cast this spell"))
    {
        return Ok(ValueExprAst::ManaSpentToCast {
            mana_filter: Some(sym.trim().to_string()),
        });
    }
    if low == "the total amount of mana paid this way"
        || low == "the life paid as it entered"
        || low == "the damage dealt"
        || low == "the other result"
        || low == "your speed"
        || low.starts_with("the amount of damage ")
    {
        return Ok(ValueExprAst::PriorResult);
    }

    // "the number of ..." / "the total number of ..."
    if let Some(after_num) = strip_prefix_any(
        &low,
        &["the total number of ", "the number of ", "number of "],
    ) {
        if after_num.ends_with(" removed this way")
            || after_num.ends_with(" exiled this way")
            || after_num.ends_with(" sacrificed this way")
            || after_num.ends_with(" discarded this way")
            || after_num.ends_with(" revealed this way")
            || after_num.ends_with(" milled this way")
            || after_num.ends_with(" that came up heads")
            || after_num.ends_with(" that came up tails")
        {
            return Ok(ValueExprAst::PriorResult);
        }
        if matches!(
            after_num,
            "basic land types among lands you control" | "basic land type among lands you control"
        ) {
            return Ok(ValueExprAst::DomainCount);
        }
        if after_num == "opponents you have" {
            return Ok(ValueExprAst::OpponentsCount);
        }
        if after_num == "card types among cards in your graveyard" {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            });
        }
        if after_num == "card types among cards in all graveyards" {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: true,
            });
        }
        if let Some(flt_raw) = after_num.strip_prefix("colors among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::ColorsAmong(Box::new(flt)));
        }
        if let Some(flt_raw) = after_num.strip_prefix("card types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
        }
        if let Some(flt_raw) = after_num.strip_prefix("creature types among ")
            && let Ok(flt) = parse_object_filter(flt_raw, vocab)
        {
            return Ok(ValueExprAst::SubtypesAmong(Box::new(flt)));
        }
        if let Some(ck_raw) =
            strip_suffix_any(&after_num, &[" counters you have", " counter you have"])
        {
            return Ok(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::You,
                counter_kind: ck_raw.trim().to_string(),
            });
        }
        if let Some(ck_raw) = after_num
            .strip_suffix(" counters your opponents have")
            .or_else(|| after_num.strip_suffix(" counter your opponents have"))
        {
            return Ok(ValueExprAst::PlayerCounterCount {
                player: PlayerRefAst::EachOpponent,
                counter_kind: ck_raw.trim().to_string(),
            });
        }
        if let Some(subj_raw) = strip_prefix_any(&after_num, &["counters on ", "counter on "])
            && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: "any".to_string(),
            });
        }
        if let Some((left, right)) = after_num.split_once(" counters on ")
            && let Ok(subj) = parse_subject_or_target(right, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: left.trim().to_string(),
            });
        }
        if let Some((left, right)) = after_num.split_once(" counter on ")
            && let Ok(subj) = parse_subject_or_target(right, vocab)
        {
            return Ok(ValueExprAst::CounterCount {
                subject: Box::new(subj),
                counter_kind: left.trim().to_string(),
            });
        }
        if let Some(rest) = after_num.strip_prefix("cards in ") {
            if let Some(who) = rest.strip_suffix(" hand") {
                return Ok(val_player_stat(
                    parse_possessive_player(who),
                    PlayerStatKind::CardsInHand,
                ));
            }
            if let Some(who) = rest.strip_suffix(" library") {
                return Ok(val_player_stat(
                    parse_possessive_player(who),
                    PlayerStatKind::CardsInLibrary,
                ));
            }
            if let Some(who) = rest.strip_suffix(" graveyard") {
                return Ok(val_player_stat(
                    parse_possessive_player(who),
                    PlayerStatKind::CardsInGraveyard,
                ));
            }
        }
        let flt_src = after_num
            .strip_prefix("differently named ")
            .unwrap_or(after_num);
        if let Ok(flt) = parse_object_filter(flt_src, vocab) {
            return Ok(ValueExprAst::Count(Box::new(flt)));
        }
    }

    // "<subject>'s power/toughness/mana value/loyalty"
    for (suffix, stat) in [
        (" power", StatKind::Power),
        (" toughness", StatKind::Toughness),
        (" mana value", StatKind::ManaValue),
        (" converted mana cost", StatKind::ManaValue),
        (" loyalty", StatKind::Loyalty),
    ] {
        if let Some(owner_raw) = low.strip_suffix(suffix) {
            if matches!(owner_raw, "its" | "his" | "her" | "their") {
                return Ok(val_obj_stat(
                    SubjectRefAst::Anaphora(owner_raw.to_string()),
                    stat,
                ));
            }
            if let Some(subj_raw) = owner_raw.strip_suffix("'s") {
                if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                    return Ok(val_obj_stat(subj, stat));
                }
                if subj_raw.starts_with("the ")
                    && (subj_raw.contains("exiled")
                        || subj_raw.contains("sacrificed")
                        || subj_raw.contains("discarded")
                        || subj_raw.contains("revealed")
                        || subj_raw.contains("milled")
                        || subj_raw.contains("returned")
                        || subj_raw.contains("destroyed"))
                {
                    return Ok(val_obj_stat(
                        SubjectRefAst::Anaphora(subj_raw.to_string()),
                        stat,
                    ));
                }
            }
        }
    }
    // "the power/toughness/mana value/loyalty of <subject>" (including "the card you exiled", "the sacrificed creature", etc.)
    for (pref, stat) in [
        ("the power of ", StatKind::Power),
        ("the toughness of ", StatKind::Toughness),
        ("the mana value of ", StatKind::ManaValue),
        ("the loyalty of ", StatKind::Loyalty),
    ] {
        if let Some(subj_raw) = low.strip_prefix(pref) {
            if let Ok(subj) = parse_subject_or_target(subj_raw, vocab) {
                return Ok(val_obj_stat(subj, stat));
            }
            if subj_raw.starts_with("the ")
                && (subj_raw.contains("exiled")
                    || subj_raw.contains("sacrificed")
                    || subj_raw.contains("discarded")
                    || subj_raw.contains("revealed")
                    || subj_raw.contains("milled")
                    || subj_raw.contains("returned")
                    || subj_raw.contains("destroyed")
                    || subj_raw.contains("other "))
            {
                return Ok(val_obj_stat(
                    SubjectRefAst::Anaphora(subj_raw.to_string()),
                    stat,
                ));
            }
        }
    }

    // "the greatest / total <stat> among/of <filter>"
    let agg_body = low.strip_prefix("the ").unwrap_or(&low);
    for (prefix, op) in [
        ("greatest ", AggregateOp::Max),
        ("total ", AggregateOp::Sum),
    ] {
        if let Some(after) = agg_body.strip_prefix(prefix) {
            for (st_str, stat) in [
                ("power ", StatKind::Power),
                ("toughness ", StatKind::Toughness),
                ("mana value ", StatKind::ManaValue),
            ] {
                if let Some(rest) = after.strip_prefix(st_str) {
                    let flt_raw = strip_prefix_any(&rest, &["among ", "of "]);
                    if let Some(fstr) = flt_raw {
                        if matches!(
                            fstr,
                            "them"
                                | "those creatures"
                                | "those permanents"
                                | "those cards"
                                | "those exiled cards"
                                | "the chosen creatures"
                                | "the exiled cards"
                                | "the sacrificed creatures"
                                | "cards exiled this way"
                        ) {
                            return Ok(ValueExprAst::AggregateStat {
                                op,
                                stat,
                                filter: Box::new(any_card_flt()),
                            });
                        }
                        if let Ok(flt) = parse_object_filter(fstr, vocab) {
                            return Ok(ValueExprAst::AggregateStat {
                                op,
                                stat,
                                filter: Box::new(flt),
                            });
                        }
                    }
                }
            }
        }
    }

    // "half <expr>[, rounded up/down]"
    if let Some(after) = low.strip_prefix("half ") {
        let (core, rounding) = if let Some(c) = after.strip_suffix(", rounded up") {
            (c, Rounding::Up)
        } else if let Some(c) = after.strip_suffix(", rounded down") {
            (c, Rounding::Down)
        } else {
            (after, Rounding::Down)
        };
        if let Some(who) = core.strip_suffix(" starting life total") {
            return Ok(val_div(
                ValueExprAst::StartingLifeTotal(parse_possessive_player(who)),
                2,
                rounding,
            ));
        }
        if let Some(who) = strip_suffix_any(&core, &[" life total", " life"]) {
            return Ok(val_div(
                val_player_stat(parse_possessive_player(who), PlayerStatKind::LifeTotal),
                2,
                rounding,
            ));
        }
        if let Ok(inner) = parse_value_expr(core, vocab) {
            return Ok(val_div(inner, 2, rounding));
        }
    }

    if let Some((op, rest)) = low
        .strip_prefix("the lowest ")
        .map(|r| (AggregateOp::Min, r))
        .or_else(|| {
            low.strip_prefix("the highest ")
                .map(|r| (AggregateOp::Max, r))
        })
        && let Some(who_raw) = rest.strip_prefix("life total among ")
    {
        let player = if who_raw.contains("opponent") {
            PlayerRefAst::EachOpponent
        } else {
            PlayerRefAst::EachPlayer
        };
        return Ok(ValueExprAst::AggregateOverPlayers {
            op,
            metric: Box::new(val_player_stat(player, PlayerStatKind::LifeTotal)),
        });
    }
    if let Some(after) = low.strip_prefix("the greatest number of ")
        && let Some(flt_raw) =
            strip_suffix_any(&after, &[" a player controls", " an opponent controls"])
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::AggregateOverPlayers {
            op: AggregateOp::Max,
            metric: Box::new(ValueExprAst::Count(Box::new(flt))),
        });
    }
    if low == "the greatest number of cards a player discarded this way" {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some((who_raw, _)) = low.split_once("'s life total")
        && let Some(p) = parse_player_ref(who_raw)
    {
        return Ok(val_player_stat(p, PlayerStatKind::LifeTotal));
    }
    if let Some(who) = low.strip_suffix(" starting life total") {
        return Ok(ValueExprAst::StartingLifeTotal(parse_possessive_player(
            who,
        )));
    }
    if let Some(who) = low.strip_suffix(" life total") {
        return Ok(val_player_stat(
            parse_possessive_player(who),
            PlayerStatKind::LifeTotal,
        ));
    }

    if let Some(flt_raw) = low.strip_prefix("the number of counters among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::Count(Box::new(flt)));
    }
    if let Some(after_ms) = low.strip_prefix("the number of ")
        && let Some((color_tok, flt_raw)) =
            after_ms.split_once(" mana symbols in the mana costs of ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::ColorPipsIn {
            color: color_tok.trim().to_string(),
            subject: Box::new(grp(flt)),
        });
    }
    if low == "the chosen number"
        || (low.starts_with("an amount of ") && low.ends_with(" of your choice"))
    {
        return Ok(ValueExprAst::ChosenX);
    }
    if (low.starts_with("the number of cards ") && low.ends_with(" this way"))
        || low.starts_with("the number of curses attached to ")
        || (low.starts_with("the number of ") && low.ends_with(" votes"))
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some(after_diff) = low.strip_prefix("the difference between ")
        && let Some((a_raw, b_raw)) = after_diff.split_once(" and ")
        && let (Ok(av), Ok(bv)) = (
            parse_value_expr(a_raw, vocab),
            parse_value_expr(b_raw, vocab),
        )
    {
        return Ok(ValueExprAst::Sub(Box::new(av), Box::new(bv)));
    }
    if matches!(
        low.as_str(),
        "the amount of mana they paid this way"
            | "the amount of mana paid this way"
            | "the amount of mana that player paid this way"
            | "the number of opponents being attacked"
            | "the number of different color pairs among permanents you control that are exactly two colors"
    ) {
        return Ok(ValueExprAst::PriorResult);
    }

    // "the greatest/least <stat> [and/or toughness] among/of <flt>"
    if let Some(rest) = low.strip_prefix("the greatest power and/or toughness among ")
        && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::ExtremeStatAmong {
            stat: StatKind::Power,
            greatest: true,
            filter: Box::new(flt),
            or_toughness: true,
        });
    }
    // "the greatest number of <flt> that have a creature type in common"
    if let Some(rest) = low
        .strip_prefix("the greatest number of ")
        .and_then(|r| r.strip_suffix(" that have a creature type in common"))
        && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
    {
        return Ok(ValueExprAst::MaxSharedSubtypeCount {
            filter: Box::new(flt),
        });
    }
    // "the number of <category> among <flt>"
    for (pref, cat) in [
        ("the number of different powers among ", "different_powers"),
        (
            "the number of different mana values among ",
            "different_mana_values",
        ),
        (
            "the number of different mana costs among ",
            "different_mana_costs",
        ),
        ("the number of permanent types among ", "permanent_types"),
        (
            "the number of planeswalker types among ",
            "planeswalker_types",
        ),
        ("the number of card types among ", "card_types"),
        ("the number of colors among ", "colors"),
        ("the number of basic land types among ", "basic_land_types"),
    ] {
        if let Some(rest) = low.strip_prefix(pref)
            && let Ok(flt) = parse_object_filter(rest.trim(), vocab)
        {
            return Ok(ValueExprAst::DistinctCountAmong {
                category: cat.to_string(),
                filter: Box::new(flt),
            });
        }
    }
    if low == "the number of colors among those creatures"
        || low == "the number of colors that creature was"
    {
        return Ok(ValueExprAst::DistinctCountAmong {
            category: "colors".to_string(),
            filter: Box::new(obj_type(CardTypeMask::CREATURE)),
        });
    }
    if low == "the number of card types the discarded card has" {
        return Ok(ValueExprAst::DistinctCountAmong {
            category: "card_types".to_string(),
            filter: Box::new(any_card_flt()),
        });
    }
    if low.starts_with("the number of abilities from among ")
        && let Some((_, among_raw)) = low.split_once(" found among ")
        && let Ok(flt) = parse_object_filter(among_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::DistinctCountAmong {
            category: "keyword_abilities".to_string(),
            filter: Box::new(flt),
        });
    }
    // "the number of <kind> counters among <flt>"
    if let Some(rest) = low.strip_prefix("the number of ")
        && let Some((ck, among_raw)) = rest.split_once(" counters among ")
        && let Ok(flt) = parse_object_filter(among_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::CounterCountAmong {
            counter_kind: ck.trim().to_string(),
            filter: Box::new(flt),
        });
    }
    // "the number of players/opponents who ..." / "being attacked" / "you attacked this combat"
    if let Some(rest) = strip_prefix_any(
        &low,
        &["the number of players ", "the number of opponents "],
    ) && (rest.starts_with("who ")
        || rest.starts_with("being ")
        || rest.starts_with("you attacked"))
    {
        return Ok(ValueExprAst::CountPlayersMatching {
            predicate: low.clone(),
        });
    }
    if low == "the number of unique vowels on that sticker" {
        return Ok(ValueExprAst::StickerUniqueVowels);
    }
    if low == "the difference between its power and toughness" {
        return Ok(ValueExprAst::AbsDiff(
            Box::new(ValueExprAst::ObjectStat {
                subject: box_anaphora("it"),
                stat: StatKind::Power,
            }),
            Box::new(ValueExprAst::ObjectStat {
                subject: box_anaphora("it"),
                stat: StatKind::Toughness,
            }),
        ));
    }
    for (prefix, stat) in [
        ("the power of ", StatKind::Power),
        ("the toughness of ", StatKind::Toughness),
        ("the mana value of ", StatKind::ManaValue),
    ] {
        if let Some(subj_raw) = low.strip_prefix(prefix) {
            let subj = parse_subject_or_target(subj_raw.trim(), vocab)
                .unwrap_or_else(|_| SubjectRefAst::Anaphora(subj_raw.trim().to_string()));
            return Ok(val_obj_stat(subj, stat));
        }
    }
    if low == "the amount of {e} you have" {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: low.clone(),
            player: Some(PlayerRefAst::You),
        });
    }
    // Compositional turn/game event & damage counters (replaces verbatim single-card literal walls)
    if let Some(tgt) = low.strip_prefix("the number of colors of mana spent to cast ")
        && matches!(tgt.trim(), "~" | "it" | "this spell" | "that spell")
    {
        return Ok(ValueExprAst::ColorsOfManaSpent);
    }
    if let Some(inner) = low
        .strip_prefix("the number of times ")
        .and_then(|r| r.strip_suffix(" was kicked"))
        && matches!(inner.trim(), "~" | "it" | "that spell" | "this spell")
    {
        return Ok(ValueExprAst::TimesKicked);
    }
    if low.starts_with("the damage dealt to ")
        || low.starts_with("the total amount of noncombat damage dealt to ")
        || low.starts_with("the greatest number noted ")
        || low.starts_with("the highest number you noted ")
        || low.starts_with("the number of spells ")
        || low.starts_with("the number of noncreature spells ")
        || low.starts_with("the number of instant and sorcery spells ")
        || low.starts_with("the number of times ")
        || low.starts_with("the number of colors in your commander")
        || low.starts_with("the number of different ")
        || low.starts_with("the number of unlocked doors ")
        || low.starts_with("the number of cards ")
            && (low.ends_with(" this turn") || low.ends_with(" this way"))
        || low.starts_with("the number of ")
            && (low.ends_with(" this turn")
                || low.ends_with(" this way")
                || low.ends_with(" this game"))
        || (low.starts_with("the number of your opponents ")
            && (low.contains(" who ") || low.contains(" with ")))
    {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: low.clone(),
            player: None,
        });
    }
    if matches!(
        low.as_str(),
        "the number of items they revealed"
            | "the high bid"
            | "the first result"
            | "the second result"
            | "the number of cards you removed from the draft with cards named ~"
            | "the total number of cards you own in exile and in your graveyard that are oozes or are named ~"
            | "the number of cards in your graveyard that are instant cards, sorcery cards, and/or have an adventure"
            | "half the highest life total among your opponents, rounded up"
            | "half the damage dealt by one of those sorcery spells this turn, rounded down"
            | "the number of nonbasic lands that creature's controller controls"
            | "the number of red mana symbols in the sacrificed creature's mana cost"
            | "twice the number of goblins it devoured"
            | "the number of colors it is"
            | "the number of colors that spell is"
            | "the life you lost when it entered"
            | "the life paid as ~ entered"
            | "the number of creatures attacking them"
            | "the number of permanents you own that your opponents control"
            | "the greatest number of stored results on it of the same value"
            | "the damage"
            | "the amount of mana that player lost this way"
            | "twice -x/-x"
            | "the life total of an opponent with the most life"
            | "the highest number you noted for cards named ~"
            | "the total power of all stickers on permanents you control"
            | "the difference between the chosen creatures' powers"
            | "the difference between those players' life totals"
            | "the greatest number of cards an opponent has drawn this turn"
            | "the power of that card or creature"
            | "the number of counters it had on it"
            | "the number of creatures those opponents control"
            | "the total number of rad counters among players"
            | "the total mana value of all cards revealed this way"
    ) {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: low.clone(),
            player: None,
        });
    }
    if let Some(after_le) = low.strip_prefix("less than or equal to ")
        && let Ok(v) = parse_value_expr(after_le.trim(), vocab)
    {
        return Ok(v);
    }

    if is_physical_tabletop_clause(&low) {
        return Ok(ValueExprAst::ManualTabletopNumber {
            prompt: raw.to_string(),
        });
    }

    Err(format!("unparsed_value:{raw}"))
}

/// Unified shared parser for `"for each <expr>"` clauses across all actions, mana specs,
/// token creators, cost modifiers, and continuous P/T buffs.
pub fn parse_for_each_value_expr(fe_raw: &str, vocab: &Vocabulary) -> Result<ValueExprAst, String> {
    let fe = trim_punct(fe_raw).to_ascii_lowercase();
    if fe.is_empty() {
        return Err("empty_for_each".to_string());
    }
    // Compositional "<expr> other than the first" -> inner - 1 (Outlaw Stitcher, etc.)
    if let Some(inner_fe) = fe.strip_suffix(" other than the first")
        && let Ok(inner) = parse_for_each_value_expr(inner_fe.trim(), vocab)
    {
        return Ok(ValueExprAst::Sub(Box::new(inner), vconst(1)));
    }
    // Compositional "mana from <source> [that was ]spent to cast (~|it|this spell|that spell)"
    if let Some(after_mf) = fe.strip_prefix("mana from ")
        && (after_mf.ends_with(" spent to cast ~")
            || after_mf.ends_with(" spent to cast it")
            || after_mf.ends_with(" spent to cast this spell")
            || after_mf.ends_with(" spent to cast that spell"))
    {
        return Ok(ValueExprAst::PriorResult);
    }
    match fe.as_str() {
        "basic land type among lands you control" | "basic land types among lands you control" => {
            return Ok(ValueExprAst::DomainCount);
        }
        "creature that left the battlefield under your control this turn" => {
            return Ok(ValueExprAst::EventCountThisTurn {
                event: "creatures_left_battlefield_under_your_control".to_string(),
                player: Some(PlayerRefAst::You),
            });
        }
        "land you had enter the battlefield under your control this turn" => {
            return Ok(ValueExprAst::EventCountThisTurn {
                event: "lands_entered_under_your_control".to_string(),
                player: Some(PlayerRefAst::You),
            });
        }
        "of your opponents who lost life this turn" => {
            return Ok(ValueExprAst::CountPlayersMatching {
                predicate: "opponents_who_lost_life_this_turn".to_string(),
            });
        }
        "color of mana spent to cast ~"
        | "color of mana spent to cast it"
        | "color of mana spent to cast this spell" => {
            return Ok(ValueExprAst::ColorsOfManaSpent);
        }
        "color of mana spent to cast that spell" => {
            return Ok(ValueExprAst::ColorsOfManaSpentOn(Box::new(anaphora(
                "that spell",
            ))));
        }
        "time ~ was kicked" | "time it was kicked" => {
            return Ok(ValueExprAst::TimesKicked);
        }
        "opponent you have" | "opponent" => {
            return Ok(ValueExprAst::OpponentsCount);
        }
        "of its colors" | "of ~'s colors" => {
            return Ok(ValueExprAst::SelfColorsCount);
        }
        "card type among cards in your graveyard" => {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: false,
            });
        }
        "card type among cards in all graveyards" => {
            return Ok(ValueExprAst::CardTypesInGraveyard {
                all_graveyards: true,
            });
        }
        "counter removed"
        | "1 life you lost"
        | "flip you won"
        | "flip you lose"
        | "creature it devoured"
        | "of that spell's colors"
        | "mana spent to cast it"
        | "mana spent to cast ~"
        | "{s} spent to cast ~"
        | "card in your opponents' hands"
        | "player being attacked"
        | "opponent dealt damage"
        | "mana from a desert spent to cast ~"
        | "mana from a treasure spent to cast it"
        | "mana from a cave spent to cast it"
        | "fellowship vote"
        | "vote they received"
        | "{e} you've paid or lost this turn"
        | "counter among players and permanents"
        | "card you own in exile and in your graveyard that's an instant card, a sorcery card, or a dragon card"
        | "opponent you're attacking"
        | "opponent you attacked this turn"
        | "target"
        | "opponent whose life total is less than half their starting life total"
        | "color in your commanders' color identity"
        | "color in your commander's color identity"
        | "time you've cast your commander from the command zone this game"
        | "time you've cast a commander from the command zone this game"
        | "graveyard with seven or more cards in it"
        | "supertype, card type, and subtype it has"
        | "time it has attacked this turn"
        | "creature blocking it beyond the first"
        | "time it regenerated this turn"
        | "name sticker on it"
        | "name sticker on ~ with seven or fewer letters"
        | "name sticker on ~ with eight or more letters"
        | "modified creature you controlled as you cast ~"
        | "creature you control with base power or toughness 1" => {
            return Ok(ValueExprAst::PriorResult);
        }
        other => {
            // Compositional color-mana symbol, prior-action "this way", and turn-history counts.
            if (other.starts_with("unspent ") && other.ends_with(" mana you have"))
                || other.ends_with(" mana symbol in its mana cost")
                || other.ends_with(" mana symbol in the mana costs of permanents you control")
                || (other.ends_with(" this way")
                    && (other.starts_with("card ")
                        || other.starts_with("creature ")
                        || other.starts_with("permanent ")
                        || other.starts_with("land ")
                        || other.starts_with("nontoken ")
                        || other.starts_with("counter ")
                        || other.starts_with("spell ")
                        || other.starts_with("damage ")
                        || other.starts_with("1 ")
                        || other.starts_with("one ")
                        || other.starts_with("{")))
            {
                return Ok(ValueExprAst::PriorResult);
            }
            if other.ends_with(" this turn")
                && !other.contains("guess")
                && !other.contains("pile")
                && (other.starts_with("card you")
                    || other.starts_with("spell you")
                    || other.starts_with("spell an opponent")
                    || other.starts_with("spell cast this turn")
                    || other.starts_with("spell a player")
                    || other.starts_with("noncreature spell ")
                    || other.starts_with("instant or sorcery spell ")
                    || other.starts_with("life you ")
                    || other.starts_with("creature ")
                    || other.starts_with("nontoken ")
                    || other.starts_with("+1/+1 counter "))
            {
                let player = if other.contains(" you ")
                    || other.contains(" you've ")
                    || other.contains(" your ")
                {
                    Some(PlayerRefAst::You)
                } else if other.contains(" an opponent ") || other.contains(" opponents ") {
                    Some(PlayerRefAst::AnyOpponent)
                } else {
                    None
                };
                return Ok(ValueExprAst::EventCountThisTurn {
                    event: other.to_string(),
                    player,
                });
            }
        }
    }

    if fe == "unspent mana you have" {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: "unspent mana you have".to_string(),
            player: Some(PlayerRefAst::You),
        });
    }
    // Compositional Wave E8 `for each ...` value expressions
    if let Some(after_dom) = strip_prefix_any(
        &fe,
        &[
            "basic land type among lands ",
            "basic land types among lands ",
        ],
    ) && let Some(who_raw) = strip_suffix_any(&after_dom, &[" control", " controls"])
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(ValueExprAst::DomainCountFor(player));
    }
    if let Some(after_life) = fe.strip_prefix("1 life ")
        && let Some(who_raw) = after_life.strip_suffix(" paid this way")
        && let Some(player) = parse_player_ref(who_raw.trim())
    {
        return Ok(ValueExprAst::LifePaidThisWay(player));
    }
    if let Some(subj_raw) = fe.strip_prefix("creature type noted for ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::NotedTypesCount(Box::new(subj)));
    }
    if let Some(after_add) = fe.strip_prefix("additional ")
        && let Some(cost_raw) = after_add.strip_suffix(" you paid")
    {
        return Ok(ValueExprAst::AdditionalCostPaidCount(
            cost_raw.trim().to_ascii_uppercase(),
        ));
    }
    if let Some(flt_raw) = fe.strip_suffix(" sacrificed this way")
        && let Ok(mut flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        flt.predicates
            .push(FilterPredicateAst::AffectedByPriorAction(
                PriorActionRefKind::SacrificedThisWay,
            ));
        return Ok(ValueExprAst::Count(Box::new(flt)));
    }
    if fe
        == "spell or ability that caused one or more players to guess or to group cards or permanents into a pile this turn"
        || (fe.starts_with("spell or ability that caused ")
            && fe.contains(" to guess ")
            && fe.ends_with(" this turn"))
    {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: "guess_or_pile_spell_or_ability".to_string(),
            player: None,
        });
    }

    // Cluster 14: `<color> mana symbol in the mana costs of <subj>` (Sanity Grinding, etc.)
    if let Some((color_part, subj_raw)) = fe.split_once(" mana symbol in the mana costs of ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ValueExprAst::ColorPipsIn {
            color: color_part.trim().to_string(),
            subject: Box::new(subj),
        });
    }
    // Cluster 14: Shortfall (`card less than two a player draws this way`, etc.)
    if let Some((_, after_lt)) = fe.split_once(" less than ")
        && after_lt.ends_with(" this way")
        && let Some((num_tok, _)) = after_lt.split_once(' ')
        && let Some(n) = parse_number_word(num_tok)
    {
        return Ok(ValueExprAst::Shortfall {
            threshold: vconst(n),
            actual: Box::new(ValueExprAst::PriorResult),
        });
    }

    if fe.ends_with(" counter removed this way")
        || fe.ends_with(" counters removed this way")
        || fe.ends_with(" counter put on it this way")
        || fe.ends_with(" counter put on ~ this way")
        || fe == "color pair"
        || fe == "creature it targets"
        || fe == "other attacking ~"
        || fe == "mountain and red card in it"
        || fe == "colored mana symbol in the milled card's mana cost"
        || (fe.contains(" counter among ") && fe.contains(" control"))
        || (fe.starts_with("card fewer than ") && fe.ends_with(" in their hand"))
        || fe.ends_with(" card exiled this way")
        || fe.ends_with(" card discarded this way")
        || fe.ends_with(" card revealed this way")
        || fe.ends_with(" card milled this way")
        || fe.ends_with(" permanent sacrificed this way")
        || fe.ends_with(" creature sacrificed this way")
        || fe.ends_with(" returned to your hand this way")
        || fe.ends_with(" returned to its owner's hand this way")
        || fe.ends_with(" returned to their owners' hands this way")
        || fe.ends_with(" returned this way")
    {
        return Ok(ValueExprAst::PriorResult);
    }

    if let Some(flt_raw) = strip_prefix_any(
        &fe,
        &[
            "different mana value among ",
            "different mana values among ",
            "different power among ",
            "different powers among ",
        ],
    ) && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
    }
    if fe == "copy"
        || fe == "flip you win"
        || fe == "card manifested this way"
        || fe == "different result"
        || fe.ends_with(" vote")
        || fe.starts_with("of those ")
        || fe.starts_with("kind of counter on ")
    {
        return Ok(ValueExprAst::PriorResult);
    }
    if let Some(flt_raw) = fe.strip_prefix("color among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::ColorsAmong(Box::new(flt)));
    }
    if let Some(flt_raw) = fe.strip_prefix("card type among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::CardTypesAmong(Box::new(flt)));
    }
    if let Some(flt_raw) = fe.strip_prefix("creature type among ")
        && let Ok(flt) = parse_object_filter(flt_raw, vocab)
    {
        return Ok(ValueExprAst::SubtypesAmong(Box::new(flt)));
    }

    // "<kind> counter(s) you have" / "<kind> counter(s) your opponents have" / "<kind> counter(s) <player> has"
    if let Some(ck) = strip_suffix_any(&fe, &[" counter you have", " counters you have"]) {
        return Ok(ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::You,
            counter_kind: ck.trim().to_string(),
        });
    }
    if let Some(ck) = strip_suffix_any(
        &fe,
        &[
            " counter your opponents have",
            " counters your opponents have",
        ],
    ) {
        return Ok(ValueExprAst::PlayerCounterCount {
            player: PlayerRefAst::EachOpponent,
            counter_kind: ck.trim().to_string(),
        });
    }
    if let Some((ck_part, who_part)) = split_once_any(&fe, &[" counter ", " counters "])
        && let Some(who_raw) = strip_suffix_any(&who_part, &[" has", " have"])
        && let Some(player) = parse_player_ref(who_raw)
    {
        return Ok(ValueExprAst::PlayerCounterCount {
            player,
            counter_kind: ck_part.trim().to_string(),
        });
    }

    // "counter(s) on <subj>" / "<kind> counter(s) on <subj>"
    if let Some(subj_raw) = strip_prefix_any(&fe, &["counter on ", "counters on "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ValueExprAst::CounterCount {
            subject: Box::new(subj),
            counter_kind: "any".to_string(),
        });
    }
    if let Some((ck, subj_raw)) = split_once_any(&fe, &[" counters on ", " counter on "])
        && let Ok(subj) = parse_subject_or_target(subj_raw, vocab)
    {
        return Ok(ValueExprAst::CounterCount {
            subject: Box::new(subj),
            counter_kind: ck.trim().to_string(),
        });
    }

    // "card in <player>'s hand/graveyard/library"
    if let Some(rest) = fe.strip_prefix("card in ") {
        if let Some(who) = rest.strip_suffix(" hand") {
            return Ok(val_player_stat(
                parse_possessive_player(who),
                PlayerStatKind::CardsInHand,
            ));
        }
        if let Some(who) = rest.strip_suffix(" graveyard") {
            return Ok(val_player_stat(
                parse_possessive_player(who),
                PlayerStatKind::CardsInGraveyard,
            ));
        }
        if let Some(who) = rest.strip_suffix(" library") {
            return Ok(val_player_stat(
                parse_possessive_player(who),
                PlayerStatKind::CardsInLibrary,
            ));
        }
    }

    // Compound "for each <A> and each <B>" (Gargantuan Leech, Alrund, etc.)
    if let Some((a, b)) = fe.split_once(" and each ")
        && let (Ok(va), Ok(vb)) = (
            parse_for_each_value_expr(a, vocab),
            parse_for_each_value_expr(b, vocab),
        )
    {
        return Ok(ValueExprAst::Add(Box::new(va), Box::new(vb)));
    }
    // "1 life <player> [have/has] lost/gained this turn" (Rakdos, Lord of Riots, Licia)
    if let Some(rest) = fe.strip_prefix("1 life ") {
        if rest == "you gained this turn" || rest == "you've gained this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: true,
            });
        }
        if rest == "you lost this turn" || rest == "you've lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::You,
                gained: false,
            });
        }
        if rest == "your opponents have lost this turn" || rest == "opponents lost this turn" {
            return Ok(ValueExprAst::LifeChangedThisTurn {
                player: PlayerRefAst::EachOpponent,
                gained: false,
            });
        }
    }
    // "opponent who <cond>" / "player who <cond>"
    if let Some(cond_raw) = strip_prefix_any(&fe, &["opponent who ", "player who "])
        && !cond_raw.is_empty()
    {
        return Ok(ValueExprAst::PriorResult);
    }
    // Wave E2 lossless "for each ..." event/property counters
    if matches!(
        fe.as_str(),
        "time it's been cast from the command zone this game"
            | "time they've cast a commander from the command zone this game"
            | "previous time you've cast ~ from the command zone this game"
            | "creature you attacked with this turn"
            | "card type among permanents you've sacrificed this turn"
            | "card you've cycled or discarded this turn"
            | "card your opponents have drawn this turn"
            | "permanent sacrificed this turn"
            | "other spell that player has cast this turn"
            | "card type they share with cards exiled with ~"
            | "of the chosen colors it is"
            | "unique vowel on that sticker"
            | "name sticker on it that begins with the chosen letter"
            | "opponent you attacked"
            | "card you put on top of your library"
            | "flip"
            | "kind of counter on ~"
            | "{g}{g} spent to cast it"
            | "different mana cost among nonland cards in your graveyard"
            | "spell or ability that caused one or more players to guess or to group cards or permanents"
            | "1 life you gained"
            | "1 damage dealt to you this turn"
            | "card type among those cards"
            | "card you put into your hand this way"
            | "counter put on a creature this way"
            | "card they revealed this way"
            | "creature chosen before it"
            | "opponent who copied the spell this way"
            | "forest and green card in it"
            | "point of bushido it has"
            | "of its creature types, to a maximum of 10"
            | "even result"
            | "green mana symbol in those cards' mana costs"
            | "black mana symbol in their mana costs"
            | "coin that comes up heads"
            | "permanent exiled this way"
            | "money vote"
            | "permanent type"
            | "different power among creatures on the battlefield"
            | "aura and equipment attached to ~"
    ) {
        return Ok(ValueExprAst::EventCountThisTurn {
            event: fe.clone(),
            player: None,
        });
    }

    let flt = parse_object_filter(&fe, vocab)?;
    Ok(ValueExprAst::Count(Box::new(flt)))
}

pub(super) fn parse_possessive_player(who: &str) -> PlayerRefAst {
    match who.trim() {
        "your" => PlayerRefAst::You,
        "an opponent's" | "target opponent's" => PlayerRefAst::TargetOpponent,
        "target player's" => PlayerRefAst::TargetPlayer,
        "each player's" => PlayerRefAst::EachPlayer,
        "each opponent's" | "opponents'" | "your opponents'" => PlayerRefAst::EachOpponent,
        "each other player's" => PlayerRefAst::EachOtherPlayer,
        "each other opponent's" => PlayerRefAst::EachOtherOpponent,
        "any player's" | "a player's" => PlayerRefAst::AnyPlayer,
        "any opponent's" => PlayerRefAst::AnyOpponent,
        "its controller's"
        | "enchanted creature's controller's"
        | "enchanted permanent's controller's"
        | "equipped creature's controller's" => PlayerRefAst::ItsController,
        "its owner's" => PlayerRefAst::ItsOwner,
        "that creature's controller's"
        | "that permanent's controller's"
        | "that spell's controller's"
        | "the exiled card's controller's" => PlayerRefAst::ThatObjectController,
        "enchanted player's" => PlayerRefAst::EnchantedPlayer,
        "defending player's" | "the defending player's" => PlayerRefAst::DefendingPlayer,
        "active player's" | "the active player's" => PlayerRefAst::ActivePlayer,
        "chosen player's" | "the chosen player's" => PlayerRefAst::ChosenPlayer,
        _ => PlayerRefAst::ThatPlayer,
    }
}
