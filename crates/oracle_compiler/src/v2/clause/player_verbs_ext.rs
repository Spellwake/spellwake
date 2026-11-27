//! Extended player verb frames, voting, choosing, melding, converting, unattaching,
//! reselecting, and manifest/cloak helpers for `v2`.
//!
//! Keeps `clause/player_verbs.rs` well below the 2,500 LOC ceiling while providing strictly-typed
//! compositional parsers for high-fanout MTG effect phrasings.

use crate::ast::{CardTypeMask, PlayerStatKind};
use crate::v2::clause::player_verbs::parse_player_verb_frame;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, CharacteristicKind, ChoiceKind, CountSpec, Duration, Effect, EntryModifiers,
    ForEachDomain, LifeOp, ObjectFilter, ObjectMutation, ObjectStateAction, PlayerActionKind,
    PlayerMutation, PlayerRef, PriorActionKind, StaticAbility, SubjectRef, SubjectStaticRule,
    ValueExpr, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::parse_value_expr;
use crate::vocab::lookup_color;

use super::{
    apply_continuous, compile_quoted_face, counted_zone_op, exact_count, move_zone_op, mutate_obj,
    mutate_player, player_action, prompt_choice, simple_zone_dest, simple_zone_op,
    zone_cards_subject,
};

/// Consume an optional capitalized proper name (e.g. `Hanweir, the Writhing Township`,
/// `~, Gaea Incarnate`, `Chittering Host`, `Brisela, Voice of Nightmares`).
pub(super) fn eat_proper_name_words(ts: &mut TokenStream<'_>) -> bool {
    let cp = ts.checkpoint();
    let mut consumed = 0usize;
    while consumed < 8 {
        if ts.eat_tilde() {
            let _ = ts.eat_word("'s");
            consumed += 1;
            let _ = ts.eat_comma();
            continue;
        }
        if let Some(w) = ts.peek_word() {
            let low = w.to_ascii_lowercase();
            let is_cap = w.chars().next().is_some_and(|c| c.is_ascii_uppercase());
            let is_connector = consumed > 0
                && matches!(
                    low.as_str(),
                    "the" | "of" | "to" | "in" | "at" | "on" | "from" | "for" | "and"
                );
            if is_cap || is_connector {
                ts.next_word();
                consumed += 1;
                let ccp = ts.checkpoint();
                if ts.eat_comma() {
                    let next_ok = ts.peek().is_some_and(|t| match t.kind {
                        TokenKind::Tilde => true,
                        TokenKind::Word(nw) => {
                            nw.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                                || nw.eq_ignore_ascii_case("the")
                        }
                        _ => false,
                    });
                    if !next_ok {
                        ts.restore(ccp);
                        break;
                    }
                }
                continue;
            }
        }
        break;
    }
    if consumed == 0 {
        ts.restore(cp);
        false
    } else {
        true
    }
}

/// Extended `choose(s)` verb frames delegated from `player_verbs.rs` before generic subject fallback.
pub(super) fn try_parse_choose_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let choose = |kind| Some(Ok((vec![prompt_choice(actor, kind)], ())));

    // 1. `choose(s) one of (those|the|their|your) piles [at random]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["one", "of", "those", "piles"],
            &["one", "of", "the", "piles"],
            &["one", "of", "their", "piles"],
            &["one", "of", "your", "piles"],
        ]) {
            let _ = ts.eat_phrase(&["at", "random"]);
            return Some(Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::ChoosePile,
                    SubjectRef::Anaphora(AnaphoraRef::OnePile),
                    Vec::new(),
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. `choose(s) one of the top <val> cards of <zone>`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "of", "the", "top"])
            && let Ok(cnt) = try_parse(ts, parse_value_expr)
            && ts.eat_any_word(&["cards", "card"])
            && ts.eat_word("of")
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return choose(ChoiceKind::ChooseObject(SubjectRef::Counted {
                filter: Box::new(ObjectFilter {
                    any_card: true,
                    zones: vec![zref],
                    ..Default::default()
                }),
                count: exact_count(cnt),
            }));
        }
        ts.restore(cp);
    }

    // 3. `choose(s) one of (the|those) (exiled|revealed) cards`
    if ts.eat_any_phrase(&[
        &["one", "of", "the", "exiled", "cards"],
        &["one", "of", "those", "exiled", "cards"],
        &["one", "of", "the", "revealed", "cards"],
        &["one", "of", "those", "revealed", "cards"],
    ]) {
        return choose(ChoiceKind::ChooseObject(SubjectRef::OneOfThem(
            ValueExpr::Const(1),
        )));
    }

    // 5. `choose(s) a [second|third] player to <player_verb_frame>`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["a", "second", "player", "to"],
            &["a", "third", "player", "to"],
            &["a", "player", "to"],
        ]) && let Ok((body, ())) =
            try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::ChosenPlayer))
        {
            return Some(Ok((
                vec![
                    prompt_choice(
                        actor,
                        ChoiceKind::ChooseObject(SubjectRef::Player(PlayerRef::AnyPlayer)),
                    ),
                    Effect::ForEach {
                        over: ForEachDomain::Player(PlayerRef::ChosenPlayer),
                        body,
                    },
                ],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 6. `choose(s) [a kind of] counter [at random] [that ~ doesn't have on it] [on <subj>] [from among <kinds>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["a", "kind", "of", "counter"], &["a", "counter"]]) {
            let _ = ts.eat_phrase(&["at", "random"]);
            let _ = ts.eat_word("that")
                && ts.eat_tilde()
                && ts.eat_phrase(&["doesn't", "have", "on", "it"]);
            let subj = if ts.eat_word("on") {
                if let Ok(s) = try_parse(ts, parse_subject_ref) {
                    let ocp = ts.checkpoint();
                    if !(ts.eat_word("or") && try_parse(ts, parse_subject_ref).is_ok()) {
                        ts.restore(ocp);
                    }
                    s
                } else {
                    SubjectRef::Anaphora(AnaphoraRef::It)
                }
            } else {
                SubjectRef::Anaphora(AnaphoraRef::It)
            };
            if ts.eat_phrase(&["from", "among"]) {
                while try_parse(ts, parse_counter_kind).is_ok() {
                    let _ = ts.eat_comma();
                    let _ = ts.eat_any_word(&["and", "or"]);
                }
            }
            return choose(ChoiceKind::ChooseObject(subj));
        }
        ts.restore(cp);
    }

    // 7. `choose(s) a number [...]` / `choose(s) 1, 2, or 3 [at random]`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "number"]) {
            if ts.eat_word("between") {
                let _ = opt(ts, parse_value_expr);
                if ts.eat_word("and") {
                    let _ = opt(ts, parse_value_expr);
                }
            } else {
                let ncp = ts.checkpoint();
                if !(ts.next_number(false).is_some()
                    && ts.eat_any_phrase(&[&["or", "greater"], &["or", "less"]]))
                {
                    ts.restore(ncp);
                }
            }
            let _ = ts.eat_phrase(&["that", "hasn't", "been", "chosen"]);
            return choose(ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number));
        }
        ts.restore(cp);
        if ts.next_number(false).is_some() && ts.eat_comma() && ts.next_number(false).is_some() {
            while ts.eat_comma() {
                if ts.eat_word("or") {
                    let _ = ts.next_number(false);
                    break;
                }
                if ts.next_number(false).is_none() {
                    break;
                }
            }
            let _ = ts.eat_phrase(&["at", "random"]);
            return choose(ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number));
        }
        ts.restore(cp);
    }

    // 10. `choose(s) and sacrifice(s) <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[&["and", "sacrifices"], &["and", "sacrifice"]])
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![simple_zone_op(
                    actor,
                    ZoneOpKind::Sacrifice,
                    subject,
                    vec![ZoneRef {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::Default,
                    }],
                    None,
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 11. `choose(s) exactly <N> <flt>` / `up to <val> <flt>` / `the same number of <flt>` / `a party from among <flt>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("exactly")
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return choose(ChoiceKind::ChooseObject(SubjectRef::Counted {
                filter: Box::new(flt),
                count: exact_count(count),
            }));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["up", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && !ts.peek_phrase(&["target"])
            && !ts.peek_phrase(&["other", "target"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && !ts.peek_phrase(&["target"])
        {
            let _ = ts.eat_any_phrase(&[&["from", "it"], &["from", "among", "them"]]);
            return choose(ChoiceKind::ChooseObject(SubjectRef::Counted {
                filter: Box::new(flt),
                count: CountSpec {
                    count,
                    up_to: true,
                    any_number: false,
                },
            }));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["the", "same", "number", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return choose(ChoiceKind::ChooseObject(SubjectRef::Counted {
                filter: Box::new(flt),
                count: exact_count(ValueExpr::PriorActionCount(PriorActionKind::ThatMany)),
            }));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["a", "party", "from", "among"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return choose(ChoiceKind::ChooseObject(SubjectRef::Group(Box::new(flt))));
        }
        ts.restore(cp);
    }

    // 12. `choose(s) from [among] <subj|it|them> <obj1> [, <obj2>, and <objN>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("from") {
            let _ = ts.eat_word("among");
            let src_ok =
                ts.eat_any_word(&["it", "them"]) || try_parse(ts, parse_subject_ref).is_ok();
            if src_ok {
                let mut objs = Vec::new();
                if ts.eat_phrase(&["a", "land", "of", "each", "basic", "land", "type"]) {
                    objs.push(SubjectRef::All(Box::new(ObjectFilter {
                        any_types: CardTypeMask::LAND,
                        ..Default::default()
                    })));
                } else {
                    while let Ok(s) = try_parse(ts, parse_subject_ref) {
                        objs.push(s);
                        let sep_cp = ts.checkpoint();
                        let had_comma = ts.eat_comma();
                        let had_and = ts.eat_word("and");
                        if (had_comma || had_and)
                            && !ts.peek_word().is_some_and(|w| {
                                matches!(
                                    w.to_ascii_lowercase().as_str(),
                                    "then" | "sacrifice" | "sacrifices" | "exile" | "exiles"
                                )
                            })
                        {
                            continue;
                        }
                        ts.restore(sep_cp);
                        break;
                    }
                }
                if !objs.is_empty() {
                    let subj = if objs.len() == 1 {
                        objs.remove(0)
                    } else {
                        SubjectRef::Compound(objs)
                    };
                    return choose(ChoiceKind::ChooseObject(subj));
                }
            }
        }
        ts.restore(cp);
    }

    // 13. Multi-object list `choose(s) <obj1>, <obj2> [, ...], and <objN> [from among <subj>]`
    {
        let cp = ts.checkpoint();
        if let Ok(first) = try_parse(ts, parse_subject_ref) {
            let mut items = vec![first];
            loop {
                let sep_cp = ts.checkpoint();
                let had_comma = ts.eat_comma();
                let had_and = ts.eat_word("and");
                if (had_comma || had_and)
                    && !ts.peek_word().is_some_and(|w| {
                        matches!(
                            w.to_ascii_lowercase().as_str(),
                            "then"
                                | "sacrifice"
                                | "sacrifices"
                                | "exile"
                                | "exiles"
                                | "put"
                                | "puts"
                                | "return"
                                | "returns"
                                | "destroy"
                                | "destroys"
                                | "tap"
                                | "taps"
                                | "untap"
                                | "untaps"
                                | "you"
                                | "they"
                                | "that"
                        )
                    })
                    && let Ok(next_s) = try_parse(ts, parse_subject_ref)
                {
                    items.push(next_s);
                    continue;
                }
                ts.restore(sep_cp);
                break;
            }
            if items.len() >= 2 {
                let fcp = ts.checkpoint();
                if !(ts.eat_phrase(&["from", "among"]) && try_parse(ts, parse_subject_ref).is_ok())
                {
                    ts.restore(fcp);
                }
                return choose(ChoiceKind::ChooseObject(SubjectRef::Compound(items)));
            }
        }
        ts.restore(cp);
    }

    // 14. `choose(s) <kw1>, <kw2>, ... or <kwN>`
    {
        let cp = ts.checkpoint();
        if let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            && kws.len() >= 2
        {
            return choose(ChoiceKind::ChooseKeywordAndGrant {
                subject: SubjectRef::Anaphora(AnaphoraRef::It),
                keywords: kws,
                duration: Duration::UntilEndOfTurn,
            });
        }
        ts.restore(cp);
    }

    // 15. Bare `choose(s) one [at random]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("one") {
            let _ = ts.eat_phrase(&["at", "random"]);
            if ts.is_eof()
                || matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Period | TokenKind::Comma)
                )
            {
                return choose(ChoiceKind::ChooseObject(SubjectRef::OneOfThem(
                    ValueExpr::Const(1),
                )));
            }
        }
        ts.restore(cp);
    }

    None
}

/// Extended player-initiated verb frames delegated from `player_verbs.rs` before its final error.
pub(super) fn try_parse_player_verb_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 4. `convert(s) <subj>`
    if ts.eat_any_word(&["convert", "converts"]) {
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::StateAction(ObjectStateAction::Transform),
                    Duration::Instant,
                )],
                (),
            )));
        }
        return Some(Err(ts.err("expected subject after `convert`")));
    }

    // 7. `reselect(s) which player [or permanent] <subj> is attacking` / `reselect(s) (its target | the targets) at random [for <subj>]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["reselect", "reselects"]) {
            let retarget = |subj| {
                Some(Ok((
                    vec![mutate_player(
                        actor,
                        PlayerMutation::Retarget {
                            subject: Box::new(subj),
                            new_target: None,
                        },
                        Duration::Instant,
                    )],
                    (),
                )))
            };
            if ts.eat_any_phrase(&[
                &["which", "player", "or", "permanent"],
                &["which", "player"],
            ]) && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["is", "attacking"])
            {
                return retarget(subj);
            }
            ts.restore(cp);
            if ts.eat_any_word(&["reselect", "reselects"])
                && ts.eat_any_phrase(&[
                    &["its", "target", "at", "random"],
                    &["the", "targets", "at", "random"],
                ])
            {
                let subj = if ts.eat_word("for") {
                    let s1 = try_parse(ts, parse_subject_ref)
                        .unwrap_or(SubjectRef::Anaphora(AnaphoraRef::ThatSpell));
                    let acp = ts.checkpoint();
                    if ts.eat_word("and")
                        && let Ok(s2) = try_parse(ts, parse_subject_ref)
                    {
                        SubjectRef::Compound(vec![s1, s2])
                    } else {
                        ts.restore(acp);
                        s1
                    }
                } else {
                    SubjectRef::Anaphora(AnaphoraRef::ThatSpell)
                };
                return retarget(subj);
            }
        }
        ts.restore(cp);
    }

    // 11. `spend(s) [<color>] mana as though it were mana of any (color|type) [to ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["spend", "spends"]) {
            if let Some(w) = ts.peek_word()
                && (lookup_color(&w.to_ascii_lowercase()).is_some()
                    || w.eq_ignore_ascii_case("colorless"))
            {
                ts.next_word();
            }
            if ts.eat_phrase(&["mana", "as", "though", "it", "were", "mana", "of", "any"])
                && ts.eat_any_word(&["color", "type"])
            {
                if ts.eat_phrase(&["to", "cast"]) {
                    let _ = ts.eat_any_phrase(&[
                        &["spells", "this", "way"],
                        &["a", "spell", "this", "way"],
                        &["that", "spell"],
                        &["those", "spells"],
                        &["it"],
                        &["them"],
                    ]);
                } else if ts.eat_phrase(&["to", "activate"]) {
                    let _ = ts.eat_any_phrase(&[
                        &["those", "abilities"],
                        &["that", "ability"],
                        &["abilities"],
                    ]);
                } else if ts.eat_phrase(&["to", "pay"]) {
                    let _ = ts.eat_any_phrase(&[
                        &["that", "spell's", "mana", "cost"],
                        &["its", "mana", "cost"],
                    ]);
                }
                return Some(Ok((
                    vec![player_action(
                        actor,
                        PlayerActionKind::SpendManaAsAnyType,
                        Duration::UntilEndOfTurn,
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 12. Generalized `manifest(s)`
    {
        let cp = ts.checkpoint();
        let manifest_op = |subject, from, count| {
            move_zone_op(
                actor,
                subject,
                from,
                simple_zone_dest(ZoneKind::Battlefield, ZoneOwner::Default),
                EntryModifiers {
                    face_down: true,
                    ..EntryModifiers::default()
                },
                count,
            )
        };
        if ts.eat_any_word(&["manifest", "manifests"]) {
            if ts.eat_phrase(&["a", "number", "of", "cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(cnt) = try_parse(ts, parse_value_expr)
            {
                return Some(Ok((
                    vec![manifest_op(
                        zone_cards_subject(zref),
                        vec![zref],
                        Some(exact_count(cnt)),
                    )],
                    (),
                )));
            }
            ts.restore(cp);
            if ts.eat_any_word(&["manifest", "manifests"]) && ts.eat_phrase(&["the", "top"]) {
                let cnt = if ts.eat_word("card") {
                    Some(ValueExpr::Const(1))
                } else if let Ok(v) = try_parse(ts, parse_value_expr)
                    && ts.eat_any_word(&["cards", "card"])
                {
                    Some(v)
                } else {
                    None
                };
                if let Some(count) = cnt
                    && ts.eat_word("of")
                    && let Ok(zref) = try_parse(ts, parse_zone_ref)
                {
                    return Some(Ok((
                        vec![manifest_op(
                            zone_cards_subject(zref),
                            vec![zref],
                            Some(exact_count(count)),
                        )],
                        (),
                    )));
                }
            }
            ts.restore(cp);
            if ts.eat_any_word(&["manifest", "manifests"])
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
            {
                let mut from = Vec::new();
                if ts.eat_word("from")
                    && let Ok(zr) = try_parse(ts, parse_zone_ref)
                {
                    from.push(zr);
                }
                return Some(Ok((vec![manifest_op(subj, from, None)], ())));
            }
        }
        ts.restore(cp);
    }

    // 13. `note(s) a creature type that hasn't been noted for ~`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["note", "notes"])
            && ts.eat_phrase(&[
                "a", "creature", "type", "that", "hasn't", "been", "noted", "for",
            ])
            && ts.eat_tilde()
        {
            return Some(Ok((
                vec![prompt_choice(
                    actor,
                    ChoiceKind::ChooseCharacteristic(CharacteristicKind::CreatureType),
                )],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 14. `repeat(s) this process [once]`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_phrase(&[
            &["repeat", "this", "process"],
            &["repeats", "this", "process"],
        ]) {
            let _ = ts.eat_word("once");
            if ts.is_eof()
                || matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Period | TokenKind::Comma)
                )
            {
                return Some(Ok((
                    vec![player_action(
                        actor,
                        PlayerActionKind::RepeatProcessOnce,
                        Duration::Instant,
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 15. `note(s) the name of <subj>`
    {
        let cp = ts.checkpoint();
        if ts.eat_any_word(&["note", "notes"])
            && ts.eat_phrase(&["the", "name", "of"])
            && let Ok(s) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![prompt_choice(actor, ChoiceKind::ChooseObject(s))],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Extended `get(s)` player frames (`get an emblem with "..." and "..."`).
pub(super) fn try_parse_get_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // Multi-ability emblem: `get(s) an emblem with "..." and "..."`
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["an", "emblem", "with"])
            && let Some(q1) = ts.next_quoted()
            && ts.eat_word("and")
            && let Some(q2) = ts.next_quoted()
        {
            let combined = format!("{q1}\n{q2}");
            if let Ok(emblem_face) = compile_quoted_face(ts, "Emblem", "Emblem", &combined) {
                return Some(Ok((
                    vec![mutate_player(
                        actor,
                        PlayerMutation::CreateEmblem(Box::new(emblem_face)),
                        Duration::Permanent,
                    )],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    None
}

/// Extended `double(s)` frames: `double your/their life total` and `double the number of each kind of counter you have`.
pub(super) fn try_parse_double_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_any_phrase(&[
        &["your", "life", "total"],
        &["their", "life", "total"],
        &["that", "player's", "life", "total"],
    ]) {
        return Some(Ok((
            vec![Effect::AdjustLife {
                player: actor,
                amount: ValueExpr::PlayerStat {
                    player: actor,
                    stat: PlayerStatKind::LifeTotal,
                },
                op: LifeOp::Gain,
            }],
            (),
        )));
    }
    if ts.eat_phrase(&["the", "number", "of", "each", "kind", "of", "counter"])
        && ts.eat_any_phrase(&[&["you", "have"], &["they", "have"]])
    {
        return Some(Ok((
            vec![mutate_obj(
                SubjectRef::Player(actor),
                ObjectMutation::DoubleCounters { kind: None },
                Duration::Instant,
            )],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Extended `have` frames delegated from `player_verbs.rs`.
pub(super) fn try_parse_have_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if let Ok(subject) = try_parse(ts, parse_subject_ref) {
        if ts.eat_word("assign")
            && ts.eat_any_word(&["its", "their"])
            && ts.eat_phrase(&["combat", "damage", "this", "turn"])
            && ts.eat_phrase(&["as", "though"])
            && ts.eat_any_word(&["it", "they"])
            && ts.eat_phrase(&["weren't", "blocked"])
        {
            return Some(Ok((
                vec![apply_continuous(
                    StaticAbility::SubjectRule {
                        subject,
                        rule: SubjectStaticRule::AssignDamageAsUnblocked,
                        condition: None,
                    },
                    Duration::UntilEndOfTurn,
                )],
                (),
            )));
        }
        if ts.eat_word("block")
            && let Ok(atk) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_any_phrase(&[
                &["this", "turn", "if", "able"],
                &["this", "combat", "if", "able"],
                &["if", "able"],
            ]);
            return Some(Ok((
                vec![mutate_obj(
                    subject,
                    ObjectMutation::MustBlockThisTurn(Some(Box::new(atk))),
                    Duration::UntilEndOfCombat,
                )],
                (),
            )));
        }
    }
    ts.restore(cp);
    let _ = actor;
    None
}

/// Extended `roll(s)` helper.
pub(super) fn try_parse_roll_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    let count_expr = if ts.eat_any_word(&["a", "an"]) {
        ValueExpr::Const(1)
    } else if let Ok(v) = try_parse(ts, parse_value_expr) {
        v
    } else {
        ts.restore(cp);
        return None;
    };
    const DIE_WORDS: &[(&[&str], u8)] = &[
        (&["four-sided", "die"], 4),
        (&["four-sided", "dice"], 4),
        (&["six-sided", "die"], 6),
        (&["six-sided", "dice"], 6),
        (&["twenty-sided", "die"], 20),
        (&["twenty-sided", "dice"], 20),
    ];
    let sides = DIE_WORDS
        .iter()
        .find_map(|&(p, s)| ts.eat_phrase(p).then_some(s))
        .or_else(|| {
            let w = ts.peek_word()?;
            let s = w
                .strip_prefix('d')
                .or_else(|| w.strip_prefix('D'))?
                .parse::<u8>()
                .ok()?;
            ts.next_word();
            Some(s)
        });
    let Some(sides) = sides else {
        ts.restore(cp);
        return None;
    };
    let mut out = if let ValueExpr::Const(c) = count_expr {
        vec![mutate_player(
            actor,
            PlayerMutation::RollDie {
                sides,
                count: u8::try_from(c.max(1)).unwrap_or(1),
            },
            Duration::Instant,
        )]
    } else {
        vec![Effect::ForEach {
            over: ForEachDomain::Count(count_expr),
            body: vec![mutate_player(
                actor,
                PlayerMutation::RollDie { sides, count: 1 },
                Duration::Instant,
            )],
        }]
    };
    if ts.eat_any_phrase(&[
        &["and", "choose", "one", "result"],
        &["and", "ignore", "the", "lower", "roll"],
        &["and", "ignore", "all", "but", "the", "highest", "roll"],
    ]) {
        out.push(prompt_choice(
            actor,
            ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number),
        ));
    }
    Some(Ok((out, ())))
}

/// Extended `from ...` zone/collection tail for `cast` and `play`.
/// Returns `Some((zone_opt, top_only))` when matched.
pub(super) fn eat_extended_cast_source(
    ts: &mut TokenStream<'_>,
) -> Option<(Option<ZoneRef>, bool)> {
    let cp = ts.checkpoint();
    if ts.eat_any_phrase(&[
        &["among", "cards", "exiled", "this", "way"],
        &["among", "the", "exiled", "cards"],
    ]) {
        return Some((
            Some(ZoneRef {
                zone: ZoneKind::Exile,
                owner: ZoneOwner::Default,
            }),
            false,
        ));
    }
    if ts.eat_any_phrase(&[
        &["among", "those", "cards"],
        &["one", "of", "those", "piles"],
        &["the", "other", "pile"],
    ]) {
        return Some((None, false));
    }
    if ts.eat_phrase(&["the", "top", "of"])
        && let Ok(zr) = try_parse(ts, parse_zone_ref)
    {
        return Some((Some(zr), true));
    }
    ts.restore(cp);
    None
}

/// Parse `play/cast <subj> ...` trailing `for as long as ...` and `mana of any type/color` riders.
pub(super) fn parse_trailing_play_mana_rider(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
    dur: Duration,
    out: &mut Vec<Effect>,
) {
    let cp = ts.checkpoint();
    let _ = ts.eat_comma();
    if ts.eat_word("and") {
        let eat_cast_obj = |s: &mut TokenStream<'_>| {
            s.eat_any_phrase(&[
                &["a", "spell", "this", "way"],
                &["spells", "this", "way"],
                &["that", "spell"],
                &["those", "spells"],
                &["it"],
                &["them"],
            ])
        };
        let after_and = ts.checkpoint();
        if let Ok(sub_actor) = try_parse(ts, crate::v2::filter::parse_player_ref)
            && matches!(sub_actor, PlayerRef::You | PlayerRef::ThatPlayer)
            && ts.eat_word("may")
            && ts.eat_word("spend")
            && ts.eat_phrase(&["mana", "as", "though", "it", "were", "mana", "of", "any"])
            && ts.eat_any_word(&["color", "type"])
            && ts.eat_phrase(&["to", "cast"])
            && eat_cast_obj(ts)
        {
            out.push(player_action(
                actor,
                PlayerActionKind::SpendManaAsAnyType,
                dur,
            ));
            return;
        }
        ts.restore(after_and);
        if ts.eat_phrase(&["mana", "of", "any"])
            && ts.eat_any_word(&["type", "color"])
            && ts.eat_phrase(&["can", "be", "spent", "to", "cast"])
            && eat_cast_obj(ts)
        {
            out.push(player_action(
                actor,
                PlayerActionKind::SpendManaAsAnyType,
                dur,
            ));
            return;
        }
        ts.restore(after_and);
        let _ = ts.eat_any_word(&["you", "they"]);
        let _ = ts.eat_word("may");
        if let Some(Ok((effs, ()))) = try_parse_player_verb_ext(ts, actor) {
            out.extend(effs);
            return;
        }
    }
    ts.restore(cp);
}

/// Parse `gain(s) life and draw(s) cards equal to <val>` tail.
pub(super) fn parse_gain_life_and_draw_tail(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> PResult<(Vec<Effect>, ())> {
    let val = parse_value_expr(ts)?;
    Ok((
        vec![
            Effect::AdjustLife {
                player: actor,
                amount: val.clone(),
                op: LifeOp::Gain,
            },
            counted_zone_op(
                actor,
                ZoneOpKind::Draw,
                SubjectRef::Player(actor),
                vec![ZoneRef {
                    zone: ZoneKind::Library,
                    owner: ZoneOwner::Default,
                }],
                Some(simple_zone_dest(ZoneKind::Hand, ZoneOwner::Default)),
                Some(exact_count(val)),
            ),
        ],
        (),
    ))
}

/// Extended `play(s)` clauses (`play [up to] <N|X|an> additional land(s)` and `play lands and cast spells from ...`).
pub(super) fn try_parse_play_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["an", "additional", "land"]) {
        let dur =
            super::subject_combat::parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Some(Ok((
            vec![mutate_player(
                actor,
                PlayerMutation::AdditionalLandPlay(1),
                dur,
            )],
            (),
        )));
    }
    let _up_to = ts.eat_phrase(&["up", "to"]);
    let land_n = ts
        .next_number(false)
        .map(|n| u8::try_from(n.max(1)).unwrap_or(1))
        .or_else(|| ts.eat_word("x").then_some(0u8));
    if let Some(n) = land_n
        && ts.eat_any_phrase(&[&["additional", "lands"], &["additional", "land"]])
    {
        let dur =
            super::subject_combat::parse_trailing_duration(ts).unwrap_or(Duration::UntilEndOfTurn);
        return Some(Ok((
            vec![mutate_player(
                actor,
                PlayerMutation::AdditionalLandPlay(n),
                dur,
            )],
            (),
        )));
    }
    ts.restore(cp);
    if ts.eat_phrase(&["lands", "and", "cast", "spells", "from"]) {
        let mut top_only = ts.eat_phrase(&["the", "top", "of"]);
        let zone_opt = if let Some((z, ext_top)) = eat_extended_cast_source(ts) {
            top_only |= ext_top;
            Some(z)
        } else {
            try_parse(ts, parse_zone_ref).ok().map(Some)
        };
        if let Some(zone) = zone_opt {
            let eat_free = |s: &mut TokenStream<'_>| {
                s.eat_any_phrase(&[
                    &["without", "paying", "their", "mana", "costs"],
                    &["without", "paying", "its", "mana", "cost"],
                ])
            };
            let _ = eat_free(ts);
            let dur = super::subject_combat::parse_trailing_duration(ts)
                .unwrap_or(Duration::UntilEndOfTurn);
            let _ = eat_free(ts);
            return Some(Ok((
                vec![apply_continuous(
                    StaticAbility::ZoneCastPermission {
                        player: actor,
                        filter: None,
                        subject: None,
                        zone,
                        mode: crate::v2::ir::CastPermissionMode::PlayFromZone {
                            top_only,
                            lands: true,
                            spells: true,
                        },
                        condition: None,
                    },
                    dur,
                )],
                (),
            )));
        }
    }
    ts.restore(cp);
    None
}

/// Extended `flip(s)` coin clauses (`flip a coin [N times | until ...]`, `flip <val> coins`).
pub(super) fn try_parse_flip_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if !ts.eat_any_word(&["flip", "flips"]) {
        return None;
    }
    let flip = player_action(actor, PlayerActionKind::FlipCoin, Duration::Instant);
    if ts.eat_phrase(&["a", "coin"]) {
        let _ = opt(ts, |s| {
            let v = s
                .next_number(false)
                .map(ValueExpr::Const)
                .map_or_else(|| parse_value_expr(s), Ok)?;
            if s.eat_word("times") {
                Ok(v)
            } else {
                Err(s.err("expected times"))
            }
        });
        let _ = ts.eat_word("or");
        if ts.eat_any_phrase(&[
            &["until", "you", "lose", "a", "flip"],
            &["until", "they", "lose", "a", "flip"],
        ]) {
            let _ = ts.eat_phrase(&["or", "choose", "to", "stop", "flipping"]);
            let wcp = ts.checkpoint();
            if !(ts.eat_comma() && ts.eat_phrase(&["whichever", "comes", "first"])) {
                ts.restore(wcp);
            }
            return Some(Ok((
                vec![Effect::Unless {
                    body: vec![flip],
                    payer: None,
                    cost: None,
                    condition: Some(crate::v2::ir::ConditionExpr::Not(Box::new(
                        crate::v2::ir::ConditionExpr::Flag(
                            crate::v2::ir::ConditionFlag::WonCoinFlip,
                        ),
                    ))),
                }],
                (),
            )));
        }
        return Some(Ok((vec![flip], ())));
    }
    if let Ok(cnt) = try_parse(ts, parse_value_expr)
        && ts.eat_word("coins")
    {
        return Some(Ok((
            vec![Effect::ForEach {
                over: ForEachDomain::Count(cnt),
                body: vec![flip],
            }],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Parse `for each other <flt> [or player] (that|the) spell [or ability] could target`.
pub(super) fn try_eat_copy_for_each_target(ts: &mut TokenStream<'_>) -> Option<ValueExpr> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["for", "each", "other"]) && try_parse(ts, parse_object_filter).is_ok() {
        let _ = ts.eat_phrase(&["or", "player"]);
        if ts.eat_any_phrase(&[
            &["that", "spell", "or", "ability", "could", "target"],
            &["the", "spell", "or", "ability", "could", "target"],
            &["that", "spell", "could", "target"],
            &["the", "spell", "could", "target"],
            &["that", "ability", "could", "target"],
        ]) {
            return Some(ValueExpr::PriorActionCount(PriorActionKind::ThatMany));
        }
    }
    ts.restore(cp);
    None
}
