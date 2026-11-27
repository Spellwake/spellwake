//! Strictly-typed [`SubjectRef`] and anaphora parser for `v2`.

use crate::ast::{CardTypeMask, ControllerRelation};
use crate::v2::filter::{parse_object_filter, parse_player_ref};
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
        || ts.eat_phrase(&["one", "or", "more", "targets"])
        || ts.eat_phrase(&["any", "number", "of", "targets"])
    {
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
                let base_target = if ts.eat_phrase(&["or", "planeswalker"]) {
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
                let base_target = if ts.eat_phrase(&["or", "planeswalker"]) {
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
                // Support trailing `or player` / `and/or player(s)` (`target creature or player`, `target creature, planeswalker, or player`, `target creatures, planeswalkers, and/or players`)
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
                // Support `<target1> and [up to <N> | another | <N>] [other] target <flt2>` (`Hull Breach`, `Relic Crush`, `Blizzard Brawl`, `Grim Contest`)
                let ccp = ts.checkpoint();
                if ts.eat_word("and")
                    && let Ok(other) = try_parse(ts, parse_trailing_target_conjunct)
                {
                    return Ok(SubjectRef::Compound(vec![first_target, other]));
                }
                ts.restore(ccp);
                return Ok(first_target);
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
        return Ok(SubjectRef::HandOf(PlayerRef::ThatPlayer));
    }
    if ts.eat_phrase(&["your", "hand"]) {
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

    // `all <filter>`
    if ts.eat_word("all") {
        if ts.eat_word("players") {
            return Ok(SubjectRef::Player(PlayerRef::EachPlayer));
        }
        let flt = parse_object_filter(ts)?;
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
            if ts.eat_phrase(&["dealt", "combat", "damage", "this", "turn", "by"]) {
                let src = parse_object_filter(ts)?;
                return Ok(SubjectRef::PlayersDealtCombatDamageBy {
                    player: p,
                    source: Box::new(src),
                    this_game: false,
                });
            }
            // Support `you and <flt>` (e.g. `You and permanents you control gain hexproof ...`)
            {
                let ycp = ts.checkpoint();
                if p == PlayerRef::You
                    && ts.eat_word("and")
                    && let Ok(extra) = try_parse(ts, parse_object_filter)
                {
                    return Ok(SubjectRef::Compound(vec![
                        SubjectRef::Player(p),
                        SubjectRef::Group(Box::new(extra)),
                    ]));
                }
                ts.restore(ycp);
            }
            // Support `<player> or a/an/another <object_filter>` (`you or another permanent you control`, `you or a Hero you control`, `the player or planeswalker it's attacking`)
            let ocp = ts.checkpoint();
            if ts.eat_word("or")
                && ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("a")
                        || w.eq_ignore_ascii_case("an")
                        || w.eq_ignore_ascii_case("another")
                        || w.eq_ignore_ascii_case("planeswalker")
                        || w.eq_ignore_ascii_case("planeswalkers")
                        || w.eq_ignore_ascii_case("battle")
                })
                && let Ok(extra) = try_parse(ts, parse_object_filter)
            {
                let _ = ts.eat_phrase(&["it's", "attacking"])
                    || ts.eat_phrase(&["they're", "attacking"]);
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

    // `one of them` / `<N> of them` / `[up to] <N> of those [cards|creatures|permanents]`
    {
        let cp = ts.checkpoint();
        let _ = ts.eat_phrase(&["up", "to"]);
        if let Some(n) = ts.next_number(false)
            && (ts.eat_phrase(&["of", "them"])
                || ts.eat_phrase(&["of", "those", "cards"])
                || ts.eat_phrase(&["of", "those", "creatures"])
                || ts.eat_phrase(&["of", "those", "permanents"])
                || ts.eat_phrase(&["of", "those"]))
        {
            return Ok(SubjectRef::OneOfThem(ValueExpr::Const(n)));
        }
        ts.restore(cp);
    }

    // `each of <subject>` (`each of up to two target creatures`) OR `each <filter>`
    if ts.eat_phrase(&["each", "of"]) {
        return parse_subject_ref(ts);
    }
    if ts.eat_word("each") {
        let flt = parse_object_filter(ts)?;
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
        return Ok(SubjectRef::Anaphora(an));
    }

    // Bare object filter group (`creatures you control`, `a Dragon creature card ...`, `a creature or player`)
    let flt = parse_object_filter(ts)?;
    if ts.eat_phrase(&["or", "player"]) || ts.eat_phrase(&["or", "a", "player"]) {
        return Ok(SubjectRef::Compound(vec![
            SubjectRef::Group(Box::new(flt)),
            SubjectRef::Player(PlayerRef::AnyPlayer),
        ]));
    }
    Ok(SubjectRef::Group(Box::new(flt)))
}

fn parse_trailing_target_conjunct(ts: &mut TokenStream<'_>) -> PResult<SubjectRef> {
    if ts.eat_phrase(&["each", "player"]) {
        return Ok(SubjectRef::Player(PlayerRef::EachPlayer));
    }
    if ts.eat_phrase(&["each", "opponent"]) {
        return Ok(SubjectRef::Player(PlayerRef::EachOpponent));
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
    let count = if ts.eat_phrase(&["up", "to"]) {
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
    if ts.eat_phrase(&["the", "rest", "of", "the", "cards"])
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
    if ts.eat_phrase(&["the", "chosen", "creature"])
        || ts.eat_phrase(&["the", "chosen", "permanent"])
        || ts.eat_phrase(&["the", "chosen", "cards"])
        || ts.eat_phrase(&["the", "chosen", "card"])
        || ts.eat_phrase(&["the", "chosen", "card"])
    {
        return Some(AnaphoraRef::ThatObject);
    }
    if ts.eat_phrase(&["the", "exiled", "card"]) {
        return Some(AnaphoraRef::TheExiledCard);
    }
    if ts.eat_phrase(&["one", "pile"]) {
        return Some(AnaphoraRef::OnePile);
    }
    if ts.eat_phrase(&["the", "other"]) {
        return Some(AnaphoraRef::OtherPile);
    }
    if ts.eat_phrase(&["that", "creature", "or", "planeswalker"])
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
    if ts.eat_word("it") || ts.eat_word("itself") || ts.eat_word("him") || ts.eat_word("her") {
        return Some(AnaphoraRef::It);
    }
    if ts.eat_word("them") {
        return Some(AnaphoraRef::Them);
    }
    None
}
