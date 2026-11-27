//! Strictly-typed [`SubjectRef`] and anaphora parser for `v2`.

use crate::ast::{CardTypeMask, ControllerRelation};
use crate::v2::filter::{eat_phrase_groups, parse_object_filter, parse_player_ref, parse_zone_ref};
use crate::v2::ir::{
    AnaphoraRef, CountSpec, FilterFlags, ObjectFilter, PlayerRef, SubjectRef, TargetCount,
    ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};

fn parse_target_player_tail(
    ts: &mut TokenStream<'_>,
    opponent_only: bool,
    count: TargetCount,
    another: bool,
) -> SubjectRef {
    let target_player = || SubjectRef::TargetPlayer {
        opponent_only,
        count,
    };
    // Oxford list: `target opponent/player, <flt1>, or <flt2>`
    {
        let ocp = ts.checkpoint();
        if ts.eat_comma()
            && let Ok(flt1) = try_parse(ts, parse_object_filter)
            && ts.eat_comma()
            && ts.eat_any_word(&["or", "and/or"])
            && let Ok(flt2) = try_parse(ts, parse_object_filter)
        {
            return SubjectRef::Compound(vec![
                target_player(),
                SubjectRef::TargetObject {
                    filter: Box::new(flt1),
                    count,
                    another,
                },
                SubjectRef::TargetObject {
                    filter: Box::new(flt2),
                    count,
                    another,
                },
            ]);
        }
        ts.restore(ocp);
    }
    let extra_type = if ts.eat_any_phrase(&[&["or", "planeswalker"], &["or", "planeswalkers"]]) {
        Some(CardTypeMask::PLANESWALKER)
    } else if ts.eat_any_phrase(&[&["or", "battle"], &["or", "battles"]]) {
        Some(CardTypeMask::BATTLE)
    } else {
        None
    };
    let base_target = if let Some(any_types) = extra_type {
        SubjectRef::Compound(vec![
            target_player(),
            SubjectRef::TargetObject {
                filter: Box::new(ObjectFilter {
                    any_types,
                    ..Default::default()
                }),
                count,
                another,
            },
        ])
    } else {
        target_player()
    };
    let ccp = ts.checkpoint();
    if ts.eat_word("and")
        && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
    {
        return SubjectRef::Compound(vec![base_target, other]);
    }
    ts.restore(ccp);
    base_target
}

/// Strictly-typed [`SubjectRef`] parser (handles `~`, `any target`, `target ...`, `up to N target ...`,
/// `another target ...`, `all ...`, `each ...`, anaphoric references, and players).
pub fn parse_subject_ref(ts: &mut TokenStream<'_>) -> PResult<SubjectRef> {
    if ts.eat_tilde() {
        let cp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
        {
            return Ok(SubjectRef::Compound(vec![SubjectRef::SelfObject, other]));
        }
        ts.restore(cp);
        return Ok(SubjectRef::SelfObject);
    }
    if ts.eat_phrase(&["any", "target"]) {
        // `any target that isn't a <subtype | commander>`
        let ex_cp = ts.checkpoint();
        if ts.eat_phrase(&["that", "isn't"]) && ts.eat_any_word(&["a", "an"]) {
            let ex_flt = if ts.eat_word("commander") {
                Some(ObjectFilter {
                    flags: FilterFlags::NOT_COMMANDER,
                    ..Default::default()
                })
            } else if let Some(sub) = ts
                .peek_word()
                .and_then(|w| ts.vocab.lookup_subtype(&w.to_ascii_lowercase()))
            {
                ts.next_word();
                Some(ObjectFilter {
                    forbidden_subtypes: vec![sub.to_string()],
                    ..Default::default()
                })
            } else {
                None
            };
            if let Some(flt) = ex_flt {
                return Ok(SubjectRef::Compound(vec![
                    SubjectRef::TargetPlayer {
                        opponent_only: false,
                        count: TargetCount::Exact(1),
                    },
                    SubjectRef::TargetObject {
                        filter: Box::new(flt),
                        count: TargetCount::Exact(1),
                        another: false,
                    },
                ]));
            }
        }
        ts.restore(ex_cp);
        return Ok(SubjectRef::AnyTarget);
    }
    if ts.eat_phrase(&["both", "creatures"]) {
        return Ok(SubjectRef::BothPairedCreatures);
    }
    // `equipped creature` / `enchanted creature` / `enchanted permanent`
    if ts.eat_phrase(&["equipped", "creature"]) {
        let ecp = ts.checkpoint();
        if ts.eat_word("or")
            && ts.peek_word().is_some_and(|w| {
                w.eq_ignore_ascii_case("a")
                    || w.eq_ignore_ascii_case("an")
                    || w.eq_ignore_ascii_case("another")
            })
            && let Ok(extra) = try_parse(ts, parse_object_filter)
        {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::Anaphora(AnaphoraRef::EquippedCreature),
                SubjectRef::Group(Box::new(extra)),
            ]));
        }
        ts.restore(ecp);
        return Ok(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature));
    }
    if ts.eat_phrase(&["enchanted", "creature"]) {
        let ecp = ts.checkpoint();
        if ts.eat_word("and")
            && ts.eat_word("other")
            && let Ok(extra) = try_parse(ts, parse_object_filter)
        {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::Anaphora(AnaphoraRef::EnchantedCreature),
                SubjectRef::Group(Box::new(extra)),
            ]));
        }
        ts.restore(ecp);
        return Ok(SubjectRef::Anaphora(AnaphoraRef::EnchantedCreature));
    }
    if ts.eat_phrase(&["enchanted", "permanent"]) {
        return Ok(SubjectRef::Anaphora(AnaphoraRef::EnchantedPermanent));
    }

    // Targeted objects & targeted players (`up to two target ...`, `another target ...`, `any number of target ...`, `target ...`)
    {
        let cp = ts.checkpoint();
        let mut another = false;
        let count_opt = if ts.eat_phrase(&["up", "to"]) {
            if ts.eat_any_phrase(&[&["x"], &["that", "many"]]) {
                Some(TargetCount::UpToX)
            } else {
                ts.next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .map(TargetCount::UpTo)
            }
        } else if ts.eat_phrase(&["one", "or", "two"]) {
            Some(TargetCount::UpTo(2))
        } else if ts.eat_any_phrase(&[&["one", "or", "more"], &["any", "number", "of"]]) {
            Some(TargetCount::AnyNumber)
        } else if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("one"))
            && matches!(ts.peek_n(1).map(|t| &t.kind), Some(TokenKind::Comma))
            && matches!(ts.peek_n(2).map(|t| &t.kind), Some(TokenKind::Word(w)) if w.eq_ignore_ascii_case("two"))
            && matches!(ts.peek_n(3).map(|t| &t.kind), Some(TokenKind::Comma))
            && matches!(ts.peek_n(4).map(|t| &t.kind), Some(TokenKind::Word(w)) if w.eq_ignore_ascii_case("or"))
            && matches!(ts.peek_n(5).map(|t| &t.kind), Some(TokenKind::Word(w)) if w.eq_ignore_ascii_case("three"))
        {
            for _ in 0..6 {
                ts.next_tok();
            }
            Some(TargetCount::UpTo(3))
        } else if ts.eat_word("another") {
            another = true;
            Some(TargetCount::Exact(1))
        } else if ts.eat_word("x") {
            Some(TargetCount::VariableX)
        } else if let Some(n) = ts.next_number(false) {
            u16::try_from(n).ok().map(TargetCount::Exact)
        } else {
            None
        };

        if ts.eat_word("other") {
            another = true;
        }
        if ts.eat_word("target") {
            let count = count_opt.unwrap_or(TargetCount::Exact(1));
            if ts.eat_any_word(&["opponent", "opponents"]) {
                return Ok(parse_target_player_tail(ts, true, count, another));
            }
            if ts.eat_any_word(&["player", "players"]) {
                return Ok(parse_target_player_tail(ts, false, count, another));
            }
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                if another {
                    flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
                }
                if ts.eat_phrase(&["chosen", "at", "random"]) {
                    flt.flags = flt.flags.union(FilterFlags::AT_RANDOM);
                }
                // Support `, player, or planeswalker`
                {
                    let pp_cp = ts.checkpoint();
                    if ts.eat_comma()
                        && ts.eat_word("player")
                        && ts.eat_comma()
                        && ts.eat_phrase(&["or", "planeswalker"])
                    {
                        return Ok(SubjectRef::Compound(vec![
                            SubjectRef::TargetObject {
                                filter: Box::new(flt),
                                count,
                                another,
                            },
                            SubjectRef::TargetPlayer {
                                opponent_only: false,
                                count,
                            },
                            SubjectRef::TargetObject {
                                filter: Box::new(ObjectFilter {
                                    any_types: CardTypeMask::PLANESWALKER,
                                    ..Default::default()
                                }),
                                count,
                                another,
                            },
                        ]));
                    }
                    ts.restore(pp_cp);
                }
                // Support trailing `or player` / `or opponent` / `and/or player(s)`
                let p_cp = ts.checkpoint();
                let _ = ts.eat_comma();
                let trailing_opp = if ts.eat_any_phrase(&[
                    &["or", "player"],
                    &["or", "players"],
                    &["and/or", "player"],
                    &["and/or", "players"],
                ]) {
                    Some(false)
                } else if ts.eat_any_phrase(&[
                    &["or", "opponent"],
                    &["or", "opponents"],
                    &["and/or", "opponent"],
                    &["and/or", "opponents"],
                ]) {
                    Some(true)
                } else {
                    None
                };
                if let Some(opponent_only) = trailing_opp {
                    return Ok(SubjectRef::Compound(vec![
                        SubjectRef::TargetObject {
                            filter: Box::new(flt),
                            count,
                            another,
                        },
                        SubjectRef::TargetPlayer {
                            opponent_only,
                            count,
                        },
                    ]));
                }
                ts.restore(p_cp);
                let first_target = SubjectRef::TargetObject {
                    filter: Box::new(flt),
                    count,
                    another,
                };
                if ts.eat_any_phrase(&[&["or", "suspended", "card"], &["or", "suspended", "cards"]])
                {
                    return Ok(SubjectRef::Compound(vec![
                        first_target,
                        SubjectRef::TargetObject {
                            filter: Box::new(ObjectFilter {
                                any_card: true,
                                flags: FilterFlags::SUSPENDED,
                                ..Default::default()
                            }),
                            count,
                            another,
                        },
                    ]));
                }
                // Support `<target1>[, <target2>, ...][,] [and | and/or] [up to <N> | another | <N>] [other] target <fltN>`
                let mut targets = vec![first_target];
                loop {
                    let ccp = ts.checkpoint();
                    let had_comma = ts.eat_comma();
                    let had_and_or = ts.eat_word("and/or");
                    let had_conj = had_and_or || ts.eat_word("and");
                    if (had_comma || had_conj)
                        && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
                    {
                        targets.push(other);
                        continue;
                    }
                    if had_and_or && let Ok(flt2) = try_parse(ts, parse_object_filter) {
                        targets.push(SubjectRef::TargetObject {
                            filter: Box::new(flt2),
                            count,
                            another,
                        });
                        continue;
                    }
                    // `target Spirit, creature with disturb, or enchantment`
                    if had_comma
                        && !had_conj
                        && let Ok(flt2) = try_parse(ts, parse_object_filter)
                        && ts.eat_comma()
                        && ts.eat_any_word(&["or", "and/or"])
                        && let Ok(flt3) = try_parse(ts, parse_object_filter)
                    {
                        targets.push(SubjectRef::TargetObject {
                            filter: Box::new(flt2),
                            count,
                            another,
                        });
                        targets.push(SubjectRef::TargetObject {
                            filter: Box::new(flt3),
                            count,
                            another,
                        });
                        continue;
                    }
                    ts.restore(ccp);
                    // `target <flt1> or <flt2>`
                    let ocp = ts.checkpoint();
                    if ts.eat_word("or")
                        && !ts.peek_word().is_some_and(|w| {
                            matches!(
                                w.to_ascii_lowercase().as_str(),
                                "pay"
                                    | "discard"
                                    | "sacrifice"
                                    | "draw"
                                    | "exile"
                                    | "destroy"
                                    | "return"
                                    | "put"
                                    | "create"
                                    | "tap"
                                    | "untap"
                                    | "gain"
                                    | "lose"
                                    | "reveal"
                                    | "shuffle"
                                    | "search"
                                    | "choose"
                                    | "counter"
                                    | "unlock"
                                    | "prevent"
                                    | "add"
                                    | "cast"
                                    | "play"
                                    | "activate"
                                    | "block"
                                    | "attack"
                            )
                        })
                        && let Ok(flt2) = try_parse(ts, parse_object_filter)
                        && (is_safe_compound_follow(ts)
                            || ts
                                .peek_word()
                                .is_some_and(|w| w.eq_ignore_ascii_case("except")))
                    {
                        targets.push(SubjectRef::TargetObject {
                            filter: Box::new(flt2),
                            count,
                            another,
                        });
                        continue;
                    }
                    ts.restore(ocp);
                    break;
                }
                if targets.len() > 1 {
                    return Ok(SubjectRef::Compound(targets));
                }
                return Ok(targets.remove(0));
            }
        }
        ts.restore(cp);
    }

    // `your commander`, `their hand`, `your hand`, `that player's hand`
    if ts.eat_phrase(&["your", "commander"]) {
        return Ok(SubjectRef::Group(Box::new(ObjectFilter {
            flags: FilterFlags::IS_COMMANDER,
            owner: Some(ControllerRelation::OwnerYou),
            ..Default::default()
        })));
    }
    let hand_owner = if ts.eat_any_phrase(&[&["their", "hand"], &["that", "player's", "hand"]]) {
        Some(PlayerRef::ThatPlayer)
    } else if ts.eat_phrase(&["your", "hand"]) {
        Some(PlayerRef::You)
    } else {
        None
    };
    if let Some(p) = hand_owner {
        let hcp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
        {
            return Ok(SubjectRef::Compound(vec![SubjectRef::HandOf(p), other]));
        }
        ts.restore(hcp);
        return Ok(SubjectRef::HandOf(p));
    }
    if ts.eat_phrase(&["target", "opponent's", "hand"]) {
        return Ok(SubjectRef::HandOf(PlayerRef::TargetOpponent));
    }
    if ts.eat_phrase(&["target", "player's", "hand"]) {
        return Ok(SubjectRef::HandOf(PlayerRef::TargetPlayer));
    }

    // `any number of <filter>`, `up to <N> <filter>`, or `<N> <filter>` (untargeted counted objects)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["any", "number", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(SubjectRef::Counted {
                filter: Box::new(flt),
                count: CountSpec {
                    count: ValueExpr::Const(0),
                    up_to: false,
                    any_number: true,
                },
            });
        }
        ts.restore(cp);
    }
    {
        let cp = ts.checkpoint();
        let up_to = ts.eat_phrase(&["up", "to"]);
        if let Some(n) = ts.next_number(false)
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Ok(SubjectRef::Counted {
                filter: Box::new(flt),
                count: CountSpec {
                    count: ValueExpr::Const(n),
                    up_to,
                    any_number: false,
                },
            });
        }
        ts.restore(cp);
    }

    // `all <filter>` (with optional guarded `and all <filter2>`)
    if ts.eat_word("all") {
        if ts.eat_word("players") {
            return Ok(SubjectRef::Player(PlayerRef::EachPlayer));
        }
        let flt = parse_object_filter(ts)?;
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["and", "all"])
            && let Ok(flt2) = try_parse(ts, parse_object_filter)
            && is_safe_compound_follow(ts)
        {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::All(Box::new(flt)),
                SubjectRef::All(Box::new(flt2)),
            ]));
        }
        ts.restore(acp);
        return Ok(SubjectRef::All(Box::new(flt)));
    }

    // `each opponent dealt combat damage this game by <flt>`
    {
        let cp = ts.checkpoint();
        if let Ok(p) = try_parse(ts, parse_player_ref) {
            // `<player> who controls <N> or more <flt>` (`a player who controls eight or more lands`)
            {
                let wcp = ts.checkpoint();
                if ts.eat_phrase(&["who", "controls"]) || ts.eat_phrase(&["who", "control"]) {
                    let min_count = ts
                        .next_number(false)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(1);
                    let _ = ts.eat_phrase(&["or", "more"]);
                    if let Ok(flt) = try_parse(ts, parse_object_filter) {
                        return Ok(SubjectRef::PlayerWhoControls {
                            player: p,
                            filter: Box::new(flt),
                            min_count,
                        });
                    }
                }
                ts.restore(wcp);
            }
            if ts.eat_phrase(&["dealt", "combat", "damage", "this", "game", "by"]) {
                let src = parse_object_filter(ts)?;
                return Ok(SubjectRef::PlayersDealtCombatDamageBy {
                    player: p,
                    source: Box::new(src),
                    this_game: true,
                });
            }
            if ts.eat_phrase(&["dealt", "combat", "damage", "this", "turn", "by"])
                || ts.eat_phrase(&["dealt", "damage", "this", "turn", "by"])
            {
                let src = parse_object_filter(ts)?;
                return Ok(SubjectRef::PlayersDealtCombatDamageBy {
                    player: p,
                    source: Box::new(src),
                    this_game: false,
                });
            }
            {
                let dd_cp = ts.checkpoint();
                if ts.eat_phrase(&["dealt", "damage", "by"])
                    && ts.eat_tilde()
                    && ts.eat_phrase(&["this", "turn"])
                {
                    return Ok(SubjectRef::PlayersDealtCombatDamageBy {
                        player: p,
                        source: Box::new(ObjectFilter::default()),
                        this_game: false,
                    });
                }
                ts.restore(dd_cp);
            }
            if ts.eat_phrase(&["dealt", "damage", "this", "way"])
                || ts.eat_phrase(&["dealt", "combat", "damage", "this", "way"])
            {
                return Ok(SubjectRef::PlayersDealtCombatDamageBy {
                    player: p,
                    source: Box::new(ObjectFilter::default()),
                    this_game: false,
                });
            }
            // Support `you and/and/or <flt>` and `you and <player_ref>`
            {
                let ycp = ts.checkpoint();
                if p == PlayerRef::You
                    && (ts.eat_word("and") || ts.eat_word("and/or"))
                    && let Ok(extra) = try_parse(ts, parse_object_filter)
                {
                    return Ok(SubjectRef::Compound(vec![
                        SubjectRef::Player(p),
                        SubjectRef::Group(Box::new(extra)),
                    ]));
                }
                ts.restore(ycp);
                if p == PlayerRef::You
                    && ts.eat_word("and")
                    && let Ok(p2) = try_parse(ts, parse_player_ref)
                    && p2 != PlayerRef::You
                    && (is_safe_compound_follow(ts)
                        || ts.peek_word().is_some_and(|w| {
                            matches!(
                                w.to_ascii_lowercase().as_str(),
                                "may"
                                    | "draw"
                                    | "gain"
                                    | "lose"
                                    | "scry"
                                    | "surveil"
                                    | "mill"
                                    | "discard"
                                    | "sacrifice"
                                    | "exile"
                                    | "create"
                                    | "put"
                                    | "reveal"
                                    | "shuffle"
                                    | "search"
                                    | "choose"
                                    | "return"
                            )
                        }))
                {
                    return Ok(SubjectRef::Compound(vec![
                        SubjectRef::Player(p),
                        SubjectRef::Player(p2),
                    ]));
                }
                ts.restore(ycp);
            }
            // Support `<player> and each <flt>` when safely terminated
            {
                let ecp = ts.checkpoint();
                if ts.eat_phrase(&["and", "each"])
                    && let Ok(extra) = try_parse(ts, parse_object_filter)
                    && is_safe_compound_follow(ts)
                {
                    return Ok(SubjectRef::Compound(vec![
                        SubjectRef::Player(p),
                        SubjectRef::Each(Box::new(extra)),
                    ]));
                }
                ts.restore(ecp);
            }
            // Support `<player> or a/an/another <object_filter>` (`you or another permanent you control`, `you or permanents you control`)
            let ocp = ts.checkpoint();
            if ts.eat_word("or")
                && ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("a")
                        || w.eq_ignore_ascii_case("an")
                        || w.eq_ignore_ascii_case("another")
                        || w.eq_ignore_ascii_case("permanent")
                        || w.eq_ignore_ascii_case("permanents")
                        || w.eq_ignore_ascii_case("creature")
                        || w.eq_ignore_ascii_case("creatures")
                        || w.eq_ignore_ascii_case("planeswalker")
                        || w.eq_ignore_ascii_case("planeswalkers")
                        || w.eq_ignore_ascii_case("battle")
                        || w.eq_ignore_ascii_case("battles")
                })
                && let Ok(extra) = try_parse(ts, parse_object_filter)
            {
                return Ok(SubjectRef::Compound(vec![
                    SubjectRef::Player(p),
                    SubjectRef::Group(Box::new(extra)),
                ]));
            }
            ts.restore(ocp);
            return Ok(SubjectRef::Player(p));
        }
        ts.restore(cp);
    }

    // `one of them` / `<N> of them` / `<N> of those/the [cards|creatures|permanents|copies|tokens]`
    // and `one of the top <N> cards of <zone>`
    {
        let cp = ts.checkpoint();
        if let Some(n) = ts.next_number(false) {
            if ts.eat_any_phrase(&[
                &["of", "them"],
                &["of", "those", "cards"],
                &["of", "those", "creatures"],
                &["of", "those", "permanents"],
                &["of", "those", "copies"],
                &["of", "the", "copies"],
                &["of", "the", "tokens"],
                &["of", "the", "chosen", "cards"],
                &["of", "the", "chosen", "creatures"],
                &["of", "the", "chosen", "permanents"],
                &["of", "those"],
            ]) {
                return Ok(SubjectRef::OneOfThem(ValueExpr::Const(n)));
            }
            if ts.eat_phrase(&["of", "the", "top"])
                && ts.next_number(false).is_some()
                && ts.eat_any_word(&["cards", "card"])
                && ts.eat_word("of")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok(SubjectRef::Counted {
                    filter: Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![zref],
                        ..Default::default()
                    }),
                    count: CountSpec {
                        count: ValueExpr::Const(n),
                        up_to: false,
                        any_number: false,
                    },
                });
            }
        }
        ts.restore(cp);
    }

    // `each of <subject>` (`each of up to two target creatures`) OR `each <filter>`
    if ts.eat_phrase(&["each", "of"]) {
        return parse_subject_ref(ts);
    }
    if ts.eat_word("each") {
        let flt = parse_object_filter(ts)?;
        let ecp = ts.checkpoint();
        if ts.eat_phrase(&["and", "each"])
            && let Ok(flt2) = try_parse(ts, parse_object_filter)
            && is_safe_compound_follow(ts)
        {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::Each(Box::new(flt)),
                SubjectRef::Each(Box::new(flt2)),
            ]));
        }
        ts.restore(ecp);
        if ts.eat_phrase(&["and", "each", "player"]) && is_safe_compound_follow(ts) {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::Each(Box::new(flt)),
                SubjectRef::Player(PlayerRef::EachPlayer),
            ]));
        }
        ts.restore(ecp);
        return Ok(SubjectRef::Each(Box::new(flt)));
    }

    // Strictly-typed anaphora references (including `that permanent or player`!)
    if ts.eat_phrase(&["that", "permanent", "or", "player"]) {
        return Ok(SubjectRef::Compound(vec![
            SubjectRef::Anaphora(AnaphoraRef::ThatPermanent),
            SubjectRef::Player(PlayerRef::AnyPlayer),
        ]));
    }
    if let Some(an) = parse_anaphora(ts) {
        if ts.eat_phrase(&["and/or", "players"]) || ts.eat_phrase(&["or", "players"]) {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::Anaphora(an),
                SubjectRef::Player(PlayerRef::EachPlayer),
            ]));
        }
        // Plural anaphora with trailing restrictive filter modifier (`those creatures with flying`,
        // `those creatures you don't control`, `those cards that has suspend`)
        if matches!(
            an,
            AnaphoraRef::ThoseCreatures
                | AnaphoraRef::ThosePermanents
                | AnaphoraRef::ThoseCards
                | AnaphoraRef::TheCopies
        ) {
            let pcp = ts.checkpoint();
            let before_pos = ts.pos;
            let mut post_flt = match an {
                AnaphoraRef::ThoseCreatures => ObjectFilter {
                    any_types: CardTypeMask::CREATURE,
                    ..Default::default()
                },
                AnaphoraRef::ThoseCards => ObjectFilter {
                    any_card: true,
                    ..Default::default()
                },
                _ => ObjectFilter {
                    any_permanent: true,
                    ..Default::default()
                },
            };
            if super::post_modifiers::parse_filter_post_modifiers(ts, &mut post_flt).is_ok()
                && ts.pos > before_pos
            {
                return Ok(SubjectRef::Group(Box::new(post_flt)));
            }
            ts.restore(pcp);
        }
        let acp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
            && is_safe_compound_follow(ts)
        {
            return Ok(SubjectRef::Compound(vec![SubjectRef::Anaphora(an), other]));
        }
        ts.restore(acp);
        return Ok(SubjectRef::Anaphora(an));
    }

    // Bare object filter group (`creatures you control`, `a Dragon creature card ...`, `a creature or player`,
    // and multi-item `an artifact, a creature, an enchantment, and a planeswalker ...`)
    let flt = parse_object_filter(ts)?;
    if ts.eat_phrase(&["or", "player"]) || ts.eat_phrase(&["or", "a", "player"]) {
        return Ok(SubjectRef::Compound(vec![
            SubjectRef::Group(Box::new(flt)),
            SubjectRef::Player(PlayerRef::AnyPlayer),
        ]));
    }
    // Checkpointed multi-item indefinite subject list (`a Cleric card, a Rogue card, a Warrior card, and/or a Wizard card ...`,
    // `an artifact, a creature, an enchantment, and a planeswalker from among ...`, `a Human you control and an artifact you control.`)
    {
        let mcp = ts.checkpoint();
        let mut items = vec![SubjectRef::Group(Box::new(flt.clone()))];
        let mut saw_conj = false;
        loop {
            let icp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_conj = ts.eat_word("and/or") || ts.eat_word("and") || ts.eat_word("or");
            if (had_comma || had_conj)
                && ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("a") || w.eq_ignore_ascii_case("an"))
                && let Ok(next_flt) = try_parse(ts, parse_object_filter)
            {
                if had_conj {
                    saw_conj = true;
                    items.push(SubjectRef::Group(Box::new(next_flt)));
                    break;
                }
                // Bare `, a/an <flt>` only allowed if continued by another `,` or `and`/`or`/`and/or`
                if matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Comma))
                    || ts.peek_word().is_some_and(|w| {
                        w.eq_ignore_ascii_case("and")
                            || w.eq_ignore_ascii_case("or")
                            || w.eq_ignore_ascii_case("and/or")
                    })
                {
                    items.push(SubjectRef::Group(Box::new(next_flt)));
                    continue;
                }
            }
            ts.restore(icp);
            break;
        }
        if saw_conj && items.len() >= 2 {
            // Optional shared post-modifier tail on the list (`from among ...`, `of their choice`, etc.)
            let mut shared = ObjectFilter::default();
            let _ = super::post_modifiers::parse_filter_post_modifiers(ts, &mut shared);
            if !shared.is_empty() {
                for item in &mut items {
                    if let SubjectRef::Group(item_flt) = item {
                        if item_flt.controller.is_none() {
                            item_flt.controller = shared.controller;
                        }
                        if item_flt.owner.is_none() {
                            item_flt.owner = shared.owner;
                        }
                        item_flt.flags = item_flt.flags.union(shared.flags);
                        item_flt.zones.extend(shared.zones.iter().copied());
                        item_flt
                            .predicates
                            .extend(shared.predicates.iter().cloned());
                    }
                }
            }
            if items.len() >= 3
                || !shared.is_empty()
                || is_safe_compound_follow(ts)
                || ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("and"))
            {
                return Ok(SubjectRef::Compound(items));
            }
        }
        ts.restore(mcp);
    }
    Ok(SubjectRef::Group(Box::new(flt)))
}

fn is_safe_compound_follow(ts: &TokenStream<'_>) -> bool {
    let Some(tok) = ts.peek() else {
        return true;
    };
    match &tok.kind {
        TokenKind::Period | TokenKind::Comma | TokenKind::Semicolon => true,
        TokenKind::Word(w) => matches!(
            w.to_ascii_lowercase().as_str(),
            "each"
                | "until"
                | "this"
                | "for"
                | "equal"
                | "get"
                | "gain"
                | "don't"
                | "can't"
                | "if"
                | "where"
                | "instead"
                | "to"
                | "onto"
                | "into"
        ),
        _ => false,
    }
}

fn parse_trailing_target_conjunct(ts: &mut TokenStream<'_>) -> PResult<SubjectRef> {
    if ts.eat_phrase(&["each", "player"]) {
        return Ok(SubjectRef::Player(PlayerRef::EachPlayer));
    }
    if ts.eat_phrase(&["each", "opponent"]) {
        return Ok(SubjectRef::Player(PlayerRef::EachOpponent));
    }
    if ts.eat_phrase(&["enchanted", "creature"]) && is_safe_compound_follow(ts) {
        return Ok(SubjectRef::Anaphora(AnaphoraRef::EnchantedCreature));
    }
    if ts.eat_phrase(&["equipped", "creature"]) && is_safe_compound_follow(ts) {
        return Ok(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature));
    }
    if ts.eat_phrase(&["that", "creature"]) && is_safe_compound_follow(ts) {
        return Ok(SubjectRef::Anaphora(AnaphoraRef::ThatCreature));
    }
    if (ts.eat_phrase(&["the", "chosen", "creatures"])
        || ts.eat_phrase(&["the", "chosen", "permanents"]))
        && is_safe_compound_follow(ts)
    {
        return Ok(SubjectRef::Anaphora(AnaphoraRef::ThosePermanents));
    }
    if (ts.eat_phrase(&["another", "target", "opponent"]) || ts.eat_phrase(&["target", "opponent"]))
        && is_safe_compound_follow(ts)
    {
        return Ok(SubjectRef::TargetPlayer {
            opponent_only: true,
            count: TargetCount::Exact(1),
        });
    }
    if (ts.eat_phrase(&["another", "target", "player"]) || ts.eat_phrase(&["target", "player"]))
        && is_safe_compound_follow(ts)
    {
        return Ok(SubjectRef::TargetPlayer {
            opponent_only: false,
            count: TargetCount::Exact(1),
        });
    }
    // `the top [<N>] card(s) of <zone>`
    {
        let tcp = ts.checkpoint();
        if ts.eat_phrase(&["the", "top"]) {
            let cnt = ts.next_number(false).unwrap_or(1);
            if (ts.eat_word("card") || ts.eat_word("cards"))
                && ts.eat_word("of")
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
            {
                return Ok(SubjectRef::Counted {
                    filter: Box::new(ObjectFilter {
                        any_card: true,
                        zones: vec![zref],
                        ..Default::default()
                    }),
                    count: CountSpec {
                        count: ValueExpr::Const(cnt),
                        up_to: false,
                        any_number: false,
                    },
                });
            }
        }
        ts.restore(tcp);
    }
    // `any number of <flt>`
    {
        let acp = ts.checkpoint();
        if ts.eat_phrase(&["any", "number", "of"])
            && !ts
                .peek_word()
                .is_some_and(|w| w.eq_ignore_ascii_case("target"))
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && is_safe_compound_follow(ts)
        {
            return Ok(SubjectRef::Counted {
                filter: Box::new(flt),
                count: CountSpec {
                    count: ValueExpr::Const(0),
                    up_to: false,
                    any_number: true,
                },
            });
        }
        ts.restore(acp);
    }
    if ts.eat_word("each") {
        let flt = parse_object_filter(ts)?;
        return Ok(SubjectRef::Each(Box::new(flt)));
    }
    if ts.eat_word("all") {
        let flt = parse_object_filter(ts)?;
        return Ok(SubjectRef::All(Box::new(flt)));
    }
    if ts
        .peek_word()
        .is_some_and(|w| w.eq_ignore_ascii_case("other"))
        && !ts.peek_n(1).is_some_and(
            |t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("target")),
        )
    {
        let flt = parse_object_filter(ts)?;
        return Ok(SubjectRef::Group(Box::new(flt)));
    }
    let mut another = false;
    let count = if ts.eat_phrase(&["any", "number", "of"]) {
        TargetCount::AnyNumber
    } else if ts.eat_phrase(&["up", "to"]) {
        if ts.eat_word("x") {
            TargetCount::UpToX
        } else {
            let n = ts
                .next_number(false)
                .and_then(|v| u16::try_from(v).ok())
                .ok_or_else(|| ts.err("expected count after `up to`"))?;
            TargetCount::UpTo(n)
        }
    } else if ts.eat_word("another") {
        another = true;
        TargetCount::Exact(1)
    } else if ts.eat_word("x") {
        TargetCount::VariableX
    } else if let Some(n) = ts.next_number(false).and_then(|v| u16::try_from(v).ok()) {
        TargetCount::Exact(n)
    } else {
        TargetCount::Exact(1)
    };
    if ts.eat_word("other") {
        another = true;
    }
    if !ts.eat_word("target") {
        return Err(ts.err("expected `target` in trailing target conjunct"));
    }
    if ts.eat_word("opponent") || ts.eat_word("opponents") {
        return Ok(SubjectRef::TargetPlayer {
            opponent_only: true,
            count,
        });
    }
    if ts.eat_word("player") || ts.eat_word("players") {
        return Ok(SubjectRef::TargetPlayer {
            opponent_only: false,
            count,
        });
    }
    let mut flt = parse_object_filter(ts)?;
    if another {
        flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
    }
    Ok(SubjectRef::TargetObject {
        filter: Box::new(flt),
        count,
        another,
    })
}

fn try_eat_dem_subtype(ts: &mut TokenStream<'_>, dem: &str) -> bool {
    let scp = ts.checkpoint();
    if ts.eat_word(dem)
        && let Some(w) = ts.peek_word()
        && w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
    {
        ts.next_word();
        let ocp = ts.checkpoint();
        if ts.eat_word("or")
            && let Some(w2) = ts.peek_word()
            && w2.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && ts.vocab.lookup_subtype(&w2.to_ascii_lowercase()).is_some()
        {
            ts.next_word();
        } else {
            ts.restore(ocp);
        }
        return true;
    }
    ts.restore(scp);
    false
}

/// Parse a strictly-typed anaphora token/phrase.
pub fn parse_anaphora(ts: &mut TokenStream<'_>) -> Option<AnaphoraRef> {
    const ANAPHORA_HEAD: &[(&[&[&str]], AnaphoraRef)] = &[
        (
            &[
                &[
                    "the", "rest", "of", "the", "cards", "revealed", "this", "way",
                ],
                &["the", "rest", "of", "the", "cards", "exiled", "this", "way"],
                &["the", "rest", "of", "the", "cards", "milled", "this", "way"],
                &[
                    "the", "rest", "of", "the", "cards", "looked", "at", "this", "way",
                ],
                &["the", "rest", "of", "the", "exiled", "cards"],
                &["the", "rest", "of", "the", "revealed", "cards"],
                &["the", "rest", "of", "the", "milled", "cards"],
                &["the", "rest", "of", "those", "exiled", "cards"],
                &["the", "rest", "of", "those", "revealed", "cards"],
                &["the", "rest", "of", "the", "cards"],
                &["the", "rest", "of", "those", "cards"],
                &["the", "rest", "of", "them"],
                &["the", "rest"],
            ],
            AnaphoraRef::TheRest,
        ),
        (
            &[
                &["the", "copies"],
                &["those", "copies"],
                &["those", "tokens"],
                &["the", "tokens"],
            ],
            AnaphoraRef::TheCopies,
        ),
        (&[&["the", "copy"], &["that", "copy"]], AnaphoraRef::TheCopy),
    ];
    if let Some(a) = eat_phrase_groups(ts, ANAPHORA_HEAD) {
        return Some(a);
    }
    let next_is_that_at_3 = ts
        .peek_n(3)
        .is_some_and(|t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("that")));
    if ts.eat_phrase(&["the", "revealed", "card"])
        || (!next_is_that_at_3 && ts.eat_phrase(&["the", "revealed", "cards"]))
        || ts.eat_phrase(&["that", "revealed", "card"])
    {
        return Some(AnaphoraRef::RevealedCard);
    }
    const ANAPHORA_MID_1: &[(&[&[&str]], AnaphoraRef)] = &[
        (
            &[&["the", "blighted", "creature"]],
            AnaphoraRef::BlightedCreature,
        ),
        (&[&["that", "source"]], AnaphoraRef::ThatSource),
        (&[&["that", "damage"]], AnaphoraRef::ThatDamage),
        (
            &[
                &["the", "sacrificed", "creature"],
                &["the", "sacrificed", "creatures"],
                &["the", "sacrificed", "permanent"],
                &["the", "sacrificed", "permanents"],
                &["the", "sacrificed", "artifact"],
                &["the", "sacrificed", "land"],
                &["the", "sacrificed", "enchantment"],
            ],
            AnaphoraRef::SacrificedObject,
        ),
        (
            &[
                &["the", "returned", "creature"],
                &["the", "returned", "card"],
                &["the", "returned", "permanent"],
            ],
            AnaphoraRef::ThatObject,
        ),
    ];
    if let Some(a) = eat_phrase_groups(ts, ANAPHORA_MID_1) {
        return Some(a);
    }
    if ts.eat_any_phrase(&[
        &["the", "exiled", "creature"],
        &["the", "exiled", "permanent"],
        &["the", "exiled", "spell"],
    ]) || (!next_is_that_at_3 && ts.eat_phrase(&["the", "exiled", "cards"]))
    {
        return Some(AnaphoraRef::ExiledObject);
    }
    const ANAPHORA_MID_2: &[(&[&[&str]], AnaphoraRef)] = &[
        (
            &[
                &["the", "discarded", "card"],
                &["the", "discarded", "cards"],
            ],
            AnaphoraRef::DiscardedObject,
        ),
        (
            &[
                &["the", "destroyed", "creature"],
                &["the", "destroyed", "permanent"],
            ],
            AnaphoraRef::DestroyedObject,
        ),
        (
            &[
                &["the", "chosen", "creatures"],
                &["the", "chosen", "permanents"],
                &["the", "chosen", "lands"],
                &["the", "chosen", "artifacts"],
                &["the", "chosen", "enchantments"],
                &["the", "chosen", "spells"],
                &["the", "chosen", "tokens"],
                &["the", "chosen", "cards"],
            ],
            AnaphoraRef::ThosePermanents,
        ),
        (
            &[
                &["the", "chosen", "creature"],
                &["the", "chosen", "permanent"],
                &["the", "chosen", "card"],
                &["that", "chosen", "creature"],
                &["that", "chosen", "permanent"],
                &["that", "chosen", "card"],
            ],
            AnaphoraRef::ThatObject,
        ),
        (
            &[&["the", "exiled", "card"], &["that", "exiled", "card"]],
            AnaphoraRef::TheExiledCard,
        ),
        (
            &[
                &["that", "exiled", "creature"],
                &["that", "exiled", "permanent"],
                &["that", "exiled", "spell"],
                &["those", "exiled", "cards"],
                &["those", "exiled", "creatures"],
                &["those", "exiled", "permanents"],
                &["the", "other", "cards", "exiled", "this", "way"],
                &["the", "other", "card", "exiled", "this", "way"],
                &["the", "other", "exiled", "cards"],
                &["the", "other", "exiled", "card"],
            ],
            AnaphoraRef::ExiledObject,
        ),
        (
            &[
                &["the", "other", "cards", "revealed", "this", "way"],
                &["the", "other", "revealed", "cards"],
                &["the", "other", "revealed", "card"],
            ],
            AnaphoraRef::RevealedCard,
        ),
        (
            &[
                &["the", "other", "cards"],
                &["the", "other", "creatures"],
                &["the", "other", "permanents"],
            ],
            AnaphoraRef::ThoseCards,
        ),
        (
            &[
                &["the", "other", "card"],
                &["the", "other", "creature"],
                &["the", "other", "permanent"],
                &["the", "other", "one"],
            ],
            AnaphoraRef::ThatObject,
        ),
        (
            &[
                &["one", "pile"],
                &["that", "pile"],
                &["the", "chosen", "pile"],
            ],
            AnaphoraRef::OnePile,
        ),
        (
            &[&["the", "other", "pile"], &["the", "other"]],
            AnaphoraRef::OtherPile,
        ),
        (
            &[
                &["that", "attacking", "creature"],
                &["that", "blocking", "creature"],
                &["that", "blocked", "creature"],
            ],
            AnaphoraRef::ThatCreature,
        ),
        (
            &[
                &["those", "attacking", "creatures"],
                &["those", "blocking", "creatures"],
            ],
            AnaphoraRef::ThoseCreatures,
        ),
        (
            &[
                &["that", "nonland", "permanent", "card"],
                &["that", "permanent", "card"],
                &["that", "creature", "card"],
                &["that", "land", "card"],
                &["that", "artifact", "card"],
                &["that", "enchantment", "card"],
            ],
            AnaphoraRef::ThatCard,
        ),
        (
            &[
                &["those", "creature", "cards"],
                &["those", "land", "cards"],
                &["those", "permanent", "cards"],
                &["those", "nonland", "cards"],
                &["those", "artifact", "cards"],
                &["those", "enchantment", "cards"],
                &["those", "revealed", "cards"],
                &["those", "milled", "cards"],
                &["those", "discarded", "cards"],
            ],
            AnaphoraRef::ThoseCards,
        ),
    ];
    if let Some(a) = eat_phrase_groups(ts, ANAPHORA_MID_2) {
        return Some(a);
    }
    // `that <CapitalizedSubtype> [or <CapitalizedSubtype>]` and `those <CapitalizedSubtype> [or <CapitalizedSubtype>]`
    if try_eat_dem_subtype(ts, "that") {
        return Some(AnaphoraRef::ThatPermanent);
    }
    if try_eat_dem_subtype(ts, "those") {
        return Some(AnaphoraRef::ThosePermanents);
    }
    const ANAPHORA_TAIL_1: &[(&[&[&str]], AnaphoraRef)] = &[
        (
            &[
                &["that", "creature", "or", "planeswalker"],
                &["that", "artifact", "or", "creature"],
                &["that", "creature", "or", "artifact"],
                &["that", "creature", "or", "enchantment"],
                &["that", "artifact", "or", "enchantment"],
                &["that", "permanent"],
                &["that", "artifact"],
                &["that", "enchantment"],
                &["that", "planeswalker"],
                &["that", "battle"],
            ],
            AnaphoraRef::ThatPermanent,
        ),
        (&[&["that", "creature"]], AnaphoraRef::ThatCreature),
        (&[&["that", "card"]], AnaphoraRef::ThatCard),
    ];
    if let Some(a) = eat_phrase_groups(ts, ANAPHORA_TAIL_1) {
        return Some(a);
    }
    if ts.peek_phrase(&["the", "card"]) && !ts.peek_n(2).is_some_and(|t| {
        matches!(
            t.kind,
            TokenKind::Word(w)
                if matches!(
                    w.to_ascii_lowercase().as_str(),
                    "you" | "you've" | "they" | "that" | "named" | "exiled" | "with" | "in" | "from"
                )
        )
    }) {
        ts.eat_phrase(&["the", "card"]);
        return Some(AnaphoraRef::ThatCard);
    }
    const ANAPHORA_TAIL_2: &[(&[&[&str]], AnaphoraRef)] = &[
        (
            &[&["that", "spell"], &["that", "ability"]],
            AnaphoraRef::ThatSpell,
        ),
        (&[&["that", "token"]], AnaphoraRef::ThatToken),
        (&[&["that", "land"]], AnaphoraRef::ThatLand),
        (&[&["those", "creatures"]], AnaphoraRef::ThoseCreatures),
        (&[&["those", "permanents"]], AnaphoraRef::ThosePermanents),
        (&[&["those", "cards"]], AnaphoraRef::ThoseCards),
        (
            &[&["it"], &["itself"], &["he"], &["she"], &["him"], &["her"]],
            AnaphoraRef::It,
        ),
        (&[&["them"]], AnaphoraRef::Them),
    ];
    eat_phrase_groups(ts, ANAPHORA_TAIL_2)
}
