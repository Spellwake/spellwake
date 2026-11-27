//! Strictly-typed [`SubjectRef`] and anaphora parser for `v2`.

use crate::ast::{CardTypeMask, ControllerRelation};
use crate::v2::filter::{parse_object_filter, parse_player_ref, parse_zone_ref};
use crate::v2::ir::{
    AnaphoraRef, CountSpec, FilterFlags, ObjectFilter, PlayerRef, SubjectRef, TargetCount,
    ValueExpr,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};

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
    if ts.eat_phrase(&["any", "target"])
        || ts.eat_phrase(&["any", "other", "target"])
        || ts.eat_phrase(&["one", "or", "two", "targets"])
        || ts.eat_phrase(&["up", "to", "two", "targets"])
        || ts.eat_phrase(&["up", "to", "three", "targets"])
        || ts.eat_phrase(&["up", "to", "x", "targets"])
        || ts.eat_phrase(&["x", "targets"])
        || ts.eat_phrase(&["one", "or", "more", "targets"])
        || ts.eat_phrase(&["any", "number", "of", "targets"])
    {
        let _ = ts.eat_phrase(&["of", "an", "opponent's", "choice"])
            || ts.eat_phrase(&["of", "defending", "player's", "choice"])
            || ts.eat_phrase(&["of", "target", "opponent's", "choice"])
            || ts.eat_phrase(&["of", "their", "choice"])
            || ts.eat_phrase(&["of", "your", "choice"]);
        // `any target that isn't a <subtype | commander>`
        let ex_cp = ts.checkpoint();
        if ts.eat_phrase(&["that", "isn't"]) && (ts.eat_word("a") || ts.eat_word("an")) {
            if ts.eat_word("commander") {
                let flt = ObjectFilter {
                    flags: FilterFlags::NOT_COMMANDER,
                    ..Default::default()
                };
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
            if let Some(sub) = ts
                .peek_word()
                .and_then(|w| ts.vocab.lookup_subtype(&w.to_ascii_lowercase()))
            {
                ts.next_word();
                let mut flt = ObjectFilter::default();
                flt.forbidden_subtypes.push(sub.to_string());
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
    {
        let tcp = ts.checkpoint();
        if ts.eat_word("one")
            && ts.eat_comma()
            && ts.eat_word("two")
            && ts.eat_comma()
            && ts.eat_phrase(&["or", "three", "targets"])
        {
            return Ok(SubjectRef::AnyTarget);
        }
        ts.restore(tcp);
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
            if ts.eat_word("x") || ts.eat_phrase(&["that", "many"]) {
                Some(TargetCount::UpToX)
            } else {
                ts.next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .map(TargetCount::UpTo)
            }
        } else if ts.eat_phrase(&["one", "or", "two"]) {
            Some(TargetCount::UpTo(2))
        } else if ts.eat_phrase(&["one", "or", "more"]) {
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
        } else if ts.eat_phrase(&["any", "number", "of"]) {
            Some(TargetCount::AnyNumber)
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
            if ts.eat_word("opponent") || ts.eat_word("opponents") {
                let _ = ts.eat_phrase(&["chosen", "at", "random"]);
                let _ = try_eat_player_relative_tail(ts);
                // Oxford list: `target opponent, creature an opponent controls, or planeswalker an opponent controls`
                {
                    let ocp = ts.checkpoint();
                    if ts.eat_comma()
                        && let Ok(flt1) = try_parse(ts, parse_object_filter)
                        && ts.eat_comma()
                        && (ts.eat_word("or") || ts.eat_word("and/or"))
                        && let Ok(flt2) = try_parse(ts, parse_object_filter)
                    {
                        return Ok(SubjectRef::Compound(vec![
                            SubjectRef::TargetPlayer {
                                opponent_only: true,
                                count,
                            },
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
                        ]));
                    }
                    ts.restore(ocp);
                }
                let base_target = if ts.eat_phrase(&["or", "planeswalker"])
                    || ts.eat_phrase(&["or", "planeswalkers"])
                {
                    SubjectRef::Compound(vec![
                        SubjectRef::TargetPlayer {
                            opponent_only: true,
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
                    ])
                } else if ts.eat_phrase(&["or", "battle"]) || ts.eat_phrase(&["or", "battles"]) {
                    SubjectRef::Compound(vec![
                        SubjectRef::TargetPlayer {
                            opponent_only: true,
                            count,
                        },
                        SubjectRef::TargetObject {
                            filter: Box::new(ObjectFilter {
                                any_types: CardTypeMask::BATTLE,
                                ..Default::default()
                            }),
                            count,
                            another,
                        },
                    ])
                } else {
                    SubjectRef::TargetPlayer {
                        opponent_only: true,
                        count,
                    }
                };
                let ccp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
                {
                    return Ok(SubjectRef::Compound(vec![base_target, other]));
                }
                ts.restore(ccp);
                return Ok(base_target);
            }
            if ts.eat_word("player") || ts.eat_word("players") {
                let _ = ts.eat_phrase(&["chosen", "at", "random"]);
                let _ = try_eat_player_relative_tail(ts);
                // Oxford list: `target player, planeswalker, or Sliver creature`
                {
                    let ocp = ts.checkpoint();
                    if ts.eat_comma()
                        && let Ok(flt1) = try_parse(ts, parse_object_filter)
                        && ts.eat_comma()
                        && (ts.eat_word("or") || ts.eat_word("and/or"))
                        && let Ok(flt2) = try_parse(ts, parse_object_filter)
                    {
                        return Ok(SubjectRef::Compound(vec![
                            SubjectRef::TargetPlayer {
                                opponent_only: false,
                                count,
                            },
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
                        ]));
                    }
                    ts.restore(ocp);
                }
                let base_target = if ts.eat_phrase(&["or", "planeswalker"])
                    || ts.eat_phrase(&["or", "planeswalkers"])
                {
                    SubjectRef::Compound(vec![
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
                    ])
                } else if ts.eat_phrase(&["or", "battle"]) || ts.eat_phrase(&["or", "battles"]) {
                    SubjectRef::Compound(vec![
                        SubjectRef::TargetPlayer {
                            opponent_only: false,
                            count,
                        },
                        SubjectRef::TargetObject {
                            filter: Box::new(ObjectFilter {
                                any_types: CardTypeMask::BATTLE,
                                ..Default::default()
                            }),
                            count,
                            another,
                        },
                    ])
                } else {
                    SubjectRef::TargetPlayer {
                        opponent_only: false,
                        count,
                    }
                };
                let ccp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
                {
                    return Ok(SubjectRef::Compound(vec![base_target, other]));
                }
                ts.restore(ccp);
                return Ok(base_target);
            }
            if let Ok(mut flt) = try_parse(ts, parse_object_filter) {
                if another {
                    flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
                }
                let _ = ts.eat_phrase(&["chosen", "at", "random"]);
                // Support `, player, or planeswalker` (`Coalborn Entity`)
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
                // Support trailing `or player` / `or opponent` / `and/or player(s)` (`target creature or player`, `target artifact, creature, planeswalker, or opponent`)
                let p_cp = ts.checkpoint();
                let _ = ts.eat_comma();
                if ts.eat_phrase(&["or", "player"])
                    || ts.eat_phrase(&["or", "players"])
                    || ts.eat_phrase(&["and/or", "player"])
                    || ts.eat_phrase(&["and/or", "players"])
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
                    ]));
                }
                if ts.eat_phrase(&["or", "opponent"])
                    || ts.eat_phrase(&["or", "opponents"])
                    || ts.eat_phrase(&["and/or", "opponent"])
                    || ts.eat_phrase(&["and/or", "opponents"])
                {
                    return Ok(SubjectRef::Compound(vec![
                        SubjectRef::TargetObject {
                            filter: Box::new(flt),
                            count,
                            another,
                        },
                        SubjectRef::TargetPlayer {
                            opponent_only: true,
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
                if ts.eat_phrase(&["or", "suspended", "card"])
                    || ts.eat_phrase(&["or", "suspended", "cards"])
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
                // (`Hull Breach`, `Relic Crush`, `Blizzard Brawl`, `Chaotic Transformation`, `Boom Box`, `Decimate`, `Casualties of War`)
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
                    // `target Spirit, creature with disturb, or enchantment` (`Thraben Exorcism`)
                    if had_comma
                        && !had_conj
                        && let Ok(flt2) = try_parse(ts, parse_object_filter)
                        && ts.eat_comma()
                        && (ts.eat_word("or") || ts.eat_word("and/or"))
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
                    // `target <flt1> or <flt2>` (`Daretti`, `Cut Short`, `Rampaging Raptor`, `Taskmaster`)
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
    if ts.eat_phrase(&["their", "hand"]) || ts.eat_phrase(&["that", "player's", "hand"]) {
        let hcp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
        {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::HandOf(PlayerRef::ThatPlayer),
                other,
            ]));
        }
        ts.restore(hcp);
        return Ok(SubjectRef::HandOf(PlayerRef::ThatPlayer));
    }
    if ts.eat_phrase(&["your", "hand"]) {
        let hcp = ts.checkpoint();
        if ts.eat_word("and")
            && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
        {
            return Ok(SubjectRef::Compound(vec![
                SubjectRef::HandOf(PlayerRef::You),
                other,
            ]));
        }
        ts.restore(hcp);
        return Ok(SubjectRef::HandOf(PlayerRef::You));
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
            // Extended restrictive player relative clauses (`who has more life than ...`, `who cast ...`, `with fewer cards in hand ...`, etc.)
            if let Some(rel_flt) = try_eat_player_relative_tail(ts) {
                return Ok(SubjectRef::PlayerWhoControls {
                    player: p,
                    filter: Box::new(rel_flt),
                    min_count: 1,
                });
            }
            // Support `you and/and/or <flt>` (e.g. `You and permanents you control gain hexproof ...`, `you and/or permanents you control`)
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
            // Support `<player> or a/an/another <object_filter>` (`you or another permanent you control`, `you or permanents you control`, `the player or planeswalker it's attacking`)
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
                let _ = ts.eat_phrase(&["it's", "attacking"])
                    || ts.eat_phrase(&["they're", "attacking"])
                    || ts.eat_phrase(&["that", "creature", "is", "attacking"])
                    || ts.eat_phrase(&["those", "creatures", "are", "attacking"])
                    || ts.eat_phrase(&["that", "player", "protects"])
                    || ts.eat_phrase(&["they", "protect"]);
                return Ok(SubjectRef::Compound(vec![
                    SubjectRef::Player(p),
                    SubjectRef::Group(Box::new(extra)),
                ]));
            }
            ts.restore(ocp);
            let _ = ts.eat_phrase(&["chosen", "at", "random"]);
            return Ok(SubjectRef::Player(p));
        }
        ts.restore(cp);
    }

    // `one of them [at random]` / `<N> of them` / `[up to] <N> of those/the [cards|creatures|permanents|copies|tokens]`
    // and `one of the top <N> cards of <zone>` (`Phyrexian Grimoire`)
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_phrase(&["up", "to"]);
        if let Some(n) = ts.next_number(false) {
            if ts.eat_phrase(&["of", "them"])
                || ts.eat_phrase(&["of", "those", "cards"])
                || ts.eat_phrase(&["of", "those", "creatures"])
                || ts.eat_phrase(&["of", "those", "permanents"])
                || ts.eat_phrase(&["of", "those", "copies"])
                || ts.eat_phrase(&["of", "the", "copies"])
                || ts.eat_phrase(&["of", "the", "tokens"])
                || ts.eat_phrase(&["of", "the", "chosen", "cards"])
                || ts.eat_phrase(&["of", "the", "chosen", "creatures"])
                || ts.eat_phrase(&["of", "the", "chosen", "permanents"])
                || ts.eat_phrase(&["of", "those"])
            {
                let _ =
                    ts.eat_phrase(&["chosen", "at", "random"]) || ts.eat_phrase(&["at", "random"]);
                return Ok(SubjectRef::OneOfThem(ValueExpr::Const(n)));
            }
            if ts.eat_phrase(&["of", "the", "top"])
                && ts.next_number(false).is_some()
                && (ts.eat_word("cards") || ts.eat_word("card"))
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
            // Optional shared post-modifier tail on the list (`from among the nonland permanents they control`, `of their choice`, `from among them`)
            let mut shared = ObjectFilter::default();
            let _ = super::post_modifiers::parse_filter_post_modifiers(ts, &mut shared);
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

fn try_eat_player_relative_tail(ts: &mut TokenStream<'_>) -> Option<ObjectFilter> {
    let cp = ts.checkpoint();
    // `<player> other than <player>`
    if ts.eat_phrase(&["other", "than"]) && try_parse(ts, parse_player_ref).is_ok() {
        return Some(ObjectFilter::default());
    }
    ts.restore(cp);
    // `<player> whose ...` (`whose coin comes up tails`, `whose life total is less than half their starting life total`,
    // `whose graveyard has fewer creature cards in it than their graveyard does and is their opponent`)
    if ts.eat_word("whose") {
        if ts.eat_phrase(&["coin", "comes", "up", "tails"])
            || ts.eat_phrase(&["coin", "comes", "up", "heads"])
            || ts.eat_phrase(&[
                "life", "total", "is", "less", "than", "half", "their", "starting", "life", "total",
            ])
        {
            return Some(ObjectFilter::default());
        }
        if ts.eat_phrase(&["graveyard", "has", "fewer"])
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["in", "it", "than", "their", "graveyard", "does"])
        {
            let _ = ts.eat_phrase(&["and", "is", "their", "opponent"]);
            return Some(ObjectFilter::default());
        }
        ts.restore(cp);
    }
    // `<player> that ~ didn't attack during your last combat` (`Territorial Hellkite`)
    if ts.eat_word("that")
        && ts.eat_tilde()
        && ts.eat_phrase(&["didn't", "attack", "during", "your", "last", "combat"])
    {
        return Some(ObjectFilter::default());
    }
    ts.restore(cp);
    // `<player> with fewer cards in hand than <player>` / `with the most life` / `with the most <flt> among your opponents` / `with exactly <N> life`
    if ts.eat_word("with") {
        if ts.eat_phrase(&["fewer", "cards", "in", "hand", "than"])
            && try_parse(ts, parse_player_ref).is_ok()
        {
            return Some(ObjectFilter::default());
        }
        ts.restore(cp);
        ts.eat_word("with");
        if ts.eat_phrase(&["the", "most", "life", "or", "tied", "for", "most", "life"])
            || ts.eat_phrase(&["the", "most", "life"])
        {
            return Some(ObjectFilter::default());
        }
        ts.restore(cp);
        ts.eat_word("with");
        if ts.eat_phrase(&["the", "most"]) && try_parse(ts, parse_object_filter).is_ok() {
            let _ = ts.eat_phrase(&["among", "your", "opponents"])
                || ts.eat_phrase(&["among", "all", "players"]);
            return Some(ObjectFilter::default());
        }
        ts.restore(cp);
        ts.eat_word("with");
        if ts.eat_word("exactly") && ts.next_number(false).is_some() && ts.eat_word("life") {
            return Some(ObjectFilter::default());
        }
        ts.restore(cp);
    }
    if !ts.eat_word("who") {
        return None;
    }
    let matched = if ts.eat_phrase(&[
        "didn't",
        "sacrifice",
        "a",
        "permanent",
        "or",
        "discard",
        "a",
        "card",
        "this",
        "way",
    ]) || ts.eat_word("does")
        || (ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("didn't"))
            && {
                let dcp = ts.checkpoint();
                ts.next_word();
                if ts.is_eof()
                    || matches!(
                        ts.peek().map(|t| &t.kind),
                        Some(
                            crate::v2::lexer::TokenKind::Period
                                | crate::v2::lexer::TokenKind::Comma
                        )
                    )
                {
                    true
                } else {
                    ts.restore(dcp);
                    false
                }
            })
        || ts.eat_phrase(&["has", "the", "initiative"])
        || ts.eat_phrase(&["have", "the", "initiative"])
        || ts.eat_phrase(&["doesn't", "have", "max", "speed"])
        || ts.eat_phrase(&["don't", "have", "max", "speed"])
        || ((ts.eat_phrase(&["doesn't", "control"]) || ts.eat_phrase(&["don't", "control"]))
            && try_parse(ts, parse_object_filter).is_ok())
        || (ts.eat_word("controlled") && try_parse(ts, parse_object_filter).is_ok())
    {
        true
    } else if ts.eat_phrase(&["was", "dealt", "combat", "damage", "by"]) {
        let _ = ts.next_number(false);
        let _ = ts.eat_phrase(&["or", "more"]);
        (ts.eat_tilde() || try_parse(ts, parse_object_filter).is_ok())
            && ts.eat_phrase(&["this", "turn"])
    } else if ts.eat_word("has") || ts.eat_word("have") {
        let _ = ts.eat_phrase(&["at", "least"]);
        let _ = ts.next_number(false);
        if (ts.eat_word("more") || ts.eat_word("fewer"))
            && (ts.eat_word("life")
                || ts.eat_phrase(&["cards", "in", "their", "hand"])
                || ts.eat_phrase(&["cards", "in", "hand"])
                || try_parse(ts, parse_object_filter).is_ok())
            && ts.eat_word("than")
            && try_parse(ts, parse_player_ref).is_ok()
        {
            let _ = ts.eat_word("do") || ts.eat_word("does");
            let _ = ts.eat_phrase(&["and", "is", "their", "opponent"]);
            true
        } else {
            false
        }
    } else if ts.eat_word("controls") || ts.eat_word("control") {
        if (ts.eat_word("more") || ts.eat_word("fewer"))
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_word("than")
            && try_parse(ts, parse_player_ref).is_ok()
        {
            let _ = ts.eat_word("do") || ts.eat_word("does");
            let _ = ts.eat_phrase(&["and", "is", "their", "opponent"]);
            true
        } else {
            ts.next_number(false).is_some()
                && ts.eat_phrase(&["or", "fewer"])
                && try_parse(ts, parse_object_filter).is_ok()
        }
    } else if ts.eat_word("cast") {
        let _ = ts.eat_phrase(&["one", "or", "more"]);
        if try_parse(ts, parse_object_filter).is_ok() {
            let _ = ts.eat_phrase(&["this", "turn"]);
            let _ = ts.eat_phrase(&["with", "the", "same", "name", "as"])
                && try_parse(ts, parse_subject_ref).is_ok();
            true
        } else {
            false
        }
    } else if ts.eat_word("attacked") {
        if ts.eat_phrase(&["this", "turn"]) {
            true
        } else if (ts.eat_word("with") && try_parse(ts, parse_object_filter).is_ok())
            || try_parse(ts, parse_subject_ref).is_ok()
        {
            let _ = ts.eat_phrase(&["this", "turn"])
                || ts.eat_phrase(&["during", "their", "last", "turn"]);
            true
        } else {
            false
        }
    } else if ts.eat_phrase(&["discarded", "their", "hand", "this", "way"])
        || ts.eat_phrase(&["discarded", "their", "hand"])
    {
        true
    } else if ts.eat_word("discarded") && try_parse(ts, parse_object_filter).is_ok() {
        let _ = ts.eat_phrase(&["this", "way"]);
        true
    } else {
        (ts.eat_word("sacrificed")
            && try_parse(ts, parse_object_filter).is_ok()
            && ts.eat_phrase(&["this", "way"]))
            || ts.eat_phrase(&["searched", "a", "library", "this", "way"])
            || ts.eat_phrase(&["searched", "their", "library", "this", "way"])
            || (ts.eat_word("shuffled")
                && try_parse(ts, parse_object_filter).is_ok()
                && ts.eat_phrase(&["into", "their", "library", "this", "way"]))
            || ts.eat_phrase(&[
                "didn't",
                "sacrifice",
                "a",
                "permanent",
                "or",
                "discard",
                "a",
                "card",
                "this",
                "way",
            ])
    };
    if matched {
        let _ = ts.eat_phrase(&["as", "you", "activate", "this", "ability"])
            || (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde());
        Some(ObjectFilter::default())
    } else {
        ts.restore(cp);
        None
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
    // `the top [<N>] card(s) of <zone>` (`Psychotic Episode`, `Become Anonymous`, `Jeskai Infiltrator`, `Suspend Aggression`, `Triumph of Saint Katherine`)
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
    // `any number of <flt>` (`Stumpsquall Hydra`: `it and any number of commanders`)
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

/// Parse a strictly-typed anaphora token/phrase.
pub fn parse_anaphora(ts: &mut TokenStream<'_>) -> Option<AnaphoraRef> {
    if ts.eat_phrase(&[
        "the", "rest", "of", "the", "cards", "revealed", "this", "way",
    ]) || ts.eat_phrase(&["the", "rest", "of", "the", "cards", "exiled", "this", "way"])
        || ts.eat_phrase(&["the", "rest", "of", "the", "cards", "milled", "this", "way"])
        || ts.eat_phrase(&[
            "the", "rest", "of", "the", "cards", "looked", "at", "this", "way",
        ])
        || ts.eat_phrase(&["the", "rest", "of", "the", "exiled", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "the", "revealed", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "the", "milled", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "those", "exiled", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "those", "revealed", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "the", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "those", "cards"])
        || ts.eat_phrase(&["the", "rest", "of", "them"])
        || ts.eat_phrase(&["the", "rest"])
    {
        return Some(AnaphoraRef::TheRest);
    }
    if ts.eat_phrase(&["the", "copies"])
        || ts.eat_phrase(&["those", "copies"])
        || ts.eat_phrase(&["those", "tokens"])
        || ts.eat_phrase(&["the", "tokens"])
    {
        return Some(AnaphoraRef::TheCopies);
    }
    if ts.eat_phrase(&["the", "copy"]) || ts.eat_phrase(&["that", "copy"]) {
        return Some(AnaphoraRef::TheCopy);
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
    if ts.eat_phrase(&["the", "blighted", "creature"]) {
        return Some(AnaphoraRef::BlightedCreature);
    }
    if ts.eat_phrase(&["that", "source"]) {
        return Some(AnaphoraRef::ThatSource);
    }
    if ts.eat_phrase(&["that", "damage"]) {
        return Some(AnaphoraRef::ThatDamage);
    }
    if ts.eat_phrase(&["the", "sacrificed", "creature"])
        || ts.eat_phrase(&["the", "sacrificed", "creatures"])
        || ts.eat_phrase(&["the", "sacrificed", "permanent"])
        || ts.eat_phrase(&["the", "sacrificed", "permanents"])
        || ts.eat_phrase(&["the", "sacrificed", "artifact"])
        || ts.eat_phrase(&["the", "sacrificed", "land"])
        || ts.eat_phrase(&["the", "sacrificed", "enchantment"])
    {
        return Some(AnaphoraRef::SacrificedObject);
    }
    if ts.eat_phrase(&["the", "returned", "creature"])
        || ts.eat_phrase(&["the", "returned", "card"])
        || ts.eat_phrase(&["the", "returned", "permanent"])
    {
        return Some(AnaphoraRef::ThatObject);
    }
    if ts.eat_phrase(&["the", "exiled", "creature"])
        || ts.eat_phrase(&["the", "exiled", "permanent"])
        || ts.eat_phrase(&["the", "exiled", "spell"])
        || (!next_is_that_at_3 && ts.eat_phrase(&["the", "exiled", "cards"]))
    {
        return Some(AnaphoraRef::ExiledObject);
    }
    if ts.eat_phrase(&["the", "discarded", "card"]) || ts.eat_phrase(&["the", "discarded", "cards"])
    {
        return Some(AnaphoraRef::DiscardedObject);
    }
    if ts.eat_phrase(&["the", "destroyed", "creature"])
        || ts.eat_phrase(&["the", "destroyed", "permanent"])
    {
        return Some(AnaphoraRef::DestroyedObject);
    }
    if ts.eat_phrase(&["the", "chosen", "creatures"])
        || ts.eat_phrase(&["the", "chosen", "permanents"])
        || ts.eat_phrase(&["the", "chosen", "lands"])
        || ts.eat_phrase(&["the", "chosen", "artifacts"])
        || ts.eat_phrase(&["the", "chosen", "enchantments"])
        || ts.eat_phrase(&["the", "chosen", "spells"])
        || ts.eat_phrase(&["the", "chosen", "tokens"])
        || ts.eat_phrase(&["the", "chosen", "cards"])
    {
        return Some(AnaphoraRef::ThosePermanents);
    }
    if ts.eat_phrase(&["the", "chosen", "creature"])
        || ts.eat_phrase(&["the", "chosen", "permanent"])
        || ts.eat_phrase(&["the", "chosen", "card"])
        || ts.eat_phrase(&["that", "chosen", "creature"])
        || ts.eat_phrase(&["that", "chosen", "permanent"])
        || ts.eat_phrase(&["that", "chosen", "card"])
    {
        return Some(AnaphoraRef::ThatObject);
    }
    if ts.eat_phrase(&["the", "exiled", "card"]) || ts.eat_phrase(&["that", "exiled", "card"]) {
        return Some(AnaphoraRef::TheExiledCard);
    }
    if ts.eat_phrase(&["that", "exiled", "creature"])
        || ts.eat_phrase(&["that", "exiled", "permanent"])
        || ts.eat_phrase(&["that", "exiled", "spell"])
        || ts.eat_phrase(&["those", "exiled", "cards"])
        || ts.eat_phrase(&["those", "exiled", "creatures"])
        || ts.eat_phrase(&["those", "exiled", "permanents"])
    {
        return Some(AnaphoraRef::ExiledObject);
    }
    if ts.eat_phrase(&["the", "other", "cards", "exiled", "this", "way"])
        || ts.eat_phrase(&["the", "other", "card", "exiled", "this", "way"])
        || ts.eat_phrase(&["the", "other", "exiled", "cards"])
        || ts.eat_phrase(&["the", "other", "exiled", "card"])
    {
        return Some(AnaphoraRef::ExiledObject);
    }
    if ts.eat_phrase(&["the", "other", "cards", "revealed", "this", "way"])
        || ts.eat_phrase(&["the", "other", "revealed", "cards"])
        || ts.eat_phrase(&["the", "other", "revealed", "card"])
    {
        return Some(AnaphoraRef::RevealedCard);
    }
    if ts.eat_phrase(&["the", "other", "cards"])
        || ts.eat_phrase(&["the", "other", "creatures"])
        || ts.eat_phrase(&["the", "other", "permanents"])
    {
        return Some(AnaphoraRef::ThoseCards);
    }
    if ts.eat_phrase(&["the", "other", "card"])
        || ts.eat_phrase(&["the", "other", "creature"])
        || ts.eat_phrase(&["the", "other", "permanent"])
        || ts.eat_phrase(&["the", "other", "one"])
    {
        return Some(AnaphoraRef::ThatObject);
    }
    if ts.eat_phrase(&["one", "pile"])
        || ts.eat_phrase(&["that", "pile"])
        || ts.eat_phrase(&["the", "chosen", "pile"])
    {
        return Some(AnaphoraRef::OnePile);
    }
    if ts.eat_phrase(&["the", "other", "pile"]) || ts.eat_phrase(&["the", "other"]) {
        return Some(AnaphoraRef::OtherPile);
    }
    if ts.eat_phrase(&["that", "attacking", "creature"])
        || ts.eat_phrase(&["that", "blocking", "creature"])
        || ts.eat_phrase(&["that", "blocked", "creature"])
    {
        return Some(AnaphoraRef::ThatCreature);
    }
    if ts.eat_phrase(&["those", "attacking", "creatures"])
        || ts.eat_phrase(&["those", "blocking", "creatures"])
    {
        return Some(AnaphoraRef::ThoseCreatures);
    }
    if ts.eat_phrase(&["that", "nonland", "permanent", "card"])
        || ts.eat_phrase(&["that", "permanent", "card"])
        || ts.eat_phrase(&["that", "creature", "card"])
        || ts.eat_phrase(&["that", "land", "card"])
        || ts.eat_phrase(&["that", "artifact", "card"])
        || ts.eat_phrase(&["that", "enchantment", "card"])
    {
        return Some(AnaphoraRef::ThatCard);
    }
    if ts.eat_phrase(&["those", "creature", "cards"])
        || ts.eat_phrase(&["those", "land", "cards"])
        || ts.eat_phrase(&["those", "permanent", "cards"])
        || ts.eat_phrase(&["those", "nonland", "cards"])
        || ts.eat_phrase(&["those", "artifact", "cards"])
        || ts.eat_phrase(&["those", "enchantment", "cards"])
        || ts.eat_phrase(&["those", "revealed", "cards"])
        || ts.eat_phrase(&["those", "milled", "cards"])
        || ts.eat_phrase(&["those", "discarded", "cards"])
    {
        return Some(AnaphoraRef::ThoseCards);
    }
    // `that <CapitalizedSubtype> [or <CapitalizedSubtype>]` and `those <CapitalizedSubtype> [or <CapitalizedSubtype>]`
    {
        let scp = ts.checkpoint();
        if ts.eat_word("that")
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
            return Some(AnaphoraRef::ThatPermanent);
        }
        ts.restore(scp);
        if ts.eat_word("those")
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
            return Some(AnaphoraRef::ThosePermanents);
        }
        ts.restore(scp);
    }
    if ts.eat_phrase(&["that", "creature", "or", "planeswalker"])
        || ts.eat_phrase(&["that", "artifact", "or", "creature"])
        || ts.eat_phrase(&["that", "creature", "or", "artifact"])
        || ts.eat_phrase(&["that", "creature", "or", "enchantment"])
        || ts.eat_phrase(&["that", "artifact", "or", "enchantment"])
        || ts.eat_phrase(&["that", "permanent"])
        || ts.eat_phrase(&["that", "artifact"])
        || ts.eat_phrase(&["that", "enchantment"])
        || ts.eat_phrase(&["that", "planeswalker"])
        || ts.eat_phrase(&["that", "battle"])
    {
        return Some(AnaphoraRef::ThatPermanent);
    }
    if ts.eat_phrase(&["that", "creature"]) {
        return Some(AnaphoraRef::ThatCreature);
    }
    if ts.eat_phrase(&["that", "card"]) {
        return Some(AnaphoraRef::ThatCard);
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
    if ts.eat_phrase(&["that", "spell"]) || ts.eat_phrase(&["that", "ability"]) {
        return Some(AnaphoraRef::ThatSpell);
    }
    if ts.eat_phrase(&["that", "token"]) {
        return Some(AnaphoraRef::ThatToken);
    }
    if ts.eat_phrase(&["that", "land"]) {
        return Some(AnaphoraRef::ThatLand);
    }
    if ts.eat_phrase(&["those", "creatures"]) {
        return Some(AnaphoraRef::ThoseCreatures);
    }
    if ts.eat_phrase(&["those", "permanents"]) {
        return Some(AnaphoraRef::ThosePermanents);
    }
    if ts.eat_phrase(&["those", "cards"]) {
        return Some(AnaphoraRef::ThoseCards);
    }
    if ts.eat_word("it")
        || ts.eat_word("itself")
        || ts.eat_word("he")
        || ts.eat_word("she")
        || ts.eat_word("him")
        || ts.eat_word("her")
    {
        return Some(AnaphoraRef::It);
    }
    if ts.eat_word("them") {
        return Some(AnaphoraRef::Them);
    }
    None
}
