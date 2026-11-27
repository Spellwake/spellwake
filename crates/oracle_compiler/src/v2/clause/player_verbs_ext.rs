//! Extended player verb frames, voting, choosing, melding, converting, unattaching,
//! reselecting, and manifest/cloak helpers for `v2`.
//!
//! Keeps `clause/player_verbs.rs` well below the 2,500 LOC ceiling while providing strictly-typed
//! compositional parsers for high-fanout MTG effect phrasings.

use crate::FaceInput;
use crate::ast::{CardTypeMask, ColorMask, PlayerStatKind};
use crate::v2::card::compile_face_v2;
use crate::v2::clause::player_verbs::parse_player_verb_frame;
use crate::v2::filter::{
    parse_counter_kind, parse_keyword_list, parse_object_filter, parse_subject_ref, parse_zone_ref,
};
use crate::v2::ir::{
    AnaphoraRef, CharacteristicKind, ChoiceKind, CountSpec, Duration, Effect, EntryModifiers,
    ForEachDomain, LifeOp, ObjectFilter, ObjectMutation, ObjectStateAction, PlayerActionKind,
    PlayerMutation, PlayerRef, PriorActionKind, StaticAbility, StaticFlagKind, SubjectRef,
    SubjectStaticRule, ValueExpr, VarName, ZoneDest, ZoneKind, ZoneOpKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt, try_parse};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};
use crate::vocab::lookup_color;

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
    // 1. `choose(s) one of (those|the|their) piles [at random]` (`Steam Augury`, `Epiphany at the Drownyard`, `Split the Spoils`, `Riddles in the Dark`, `Whims of the Fates`, `Curator of Destinies`, `Hostile Negotiations`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "of", "those", "piles"])
            || ts.eat_phrase(&["one", "of", "the", "piles"])
            || ts.eat_phrase(&["one", "of", "their", "piles"])
            || ts.eat_phrase(&["one", "of", "your", "piles"])
        {
            let _ = ts.eat_phrase(&["at", "random"]);
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::ChoosePile,
                    subject: SubjectRef::Anaphora(AnaphoraRef::OnePile),
                    from: Vec::new(),
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. `choose(s) one of the top <val> cards of <zone>` (`Phyrexian Grimoire`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "of", "the", "top"])
            && let Ok(cnt) = try_parse(ts, parse_value_expr)
            && (ts.eat_word("cards") || ts.eat_word("card"))
            && ts.eat_word("of")
            && let Ok(zref) = try_parse(ts, parse_zone_ref)
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(SubjectRef::Counted {
                        filter: Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zref],
                            ..Default::default()
                        }),
                        count: CountSpec {
                            count: cnt,
                            up_to: false,
                            any_number: false,
                        },
                    }),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 3. `choose(s) one of (the|those) (exiled|revealed) cards` (`Coin of Fate`, `Thieves' Auction`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["one", "of", "the", "exiled", "cards"])
            || ts.eat_phrase(&["one", "of", "those", "exiled", "cards"])
            || ts.eat_phrase(&["one", "of", "the", "revealed", "cards"])
            || ts.eat_phrase(&["one", "of", "those", "revealed", "cards"])
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(SubjectRef::OneOfThem(ValueExpr::Const(1))),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 4. `choose(s) one result` (`Valiant/Arcane/Grave/Wild/Reckless Endeavor`)
    if ts.eat_phrase(&["one", "result"]) {
        return Some(Ok((
            vec![Effect::PromptChoice {
                chooser: actor,
                kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number),
            }],
            (),
        )));
    }

    // 5. `choose(s) a [second|third] player to <player_verb_frame>` (`Gluntch, the Bestower`)
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["a", "second", "player", "to"])
            || ts.eat_phrase(&["a", "third", "player", "to"])
            || ts.eat_phrase(&["a", "player", "to"]))
            && let Ok((body, ())) =
                try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::ChosenPlayer))
        {
            return Some(Ok((
                vec![
                    Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseObject(SubjectRef::Player(PlayerRef::AnyPlayer)),
                    },
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
    //    (`Clockspinning`, `The Caves of Androzani`, `Contractual Safeguard`, `Ichormoon Gauntlet`, `Aven Courier`, `Animation Module`, `Crystalline Giant`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["a", "kind", "of", "counter"]) || ts.eat_phrase(&["a", "counter"]) {
            let _ = ts.eat_phrase(&["at", "random"]);
            let _ = ts.eat_word("that")
                && ts.eat_tilde()
                && ts.eat_phrase(&["doesn't", "have", "on", "it"]);
            let subj = if ts.eat_word("on") {
                if let Ok(s) = try_parse(ts, parse_subject_ref) {
                    let ocp = ts.checkpoint();
                    if ts.eat_word("or") && try_parse(ts, parse_subject_ref).is_ok() {
                        // `target permanent or suspended card` / `target permanent or player`
                    } else {
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
                    let _ = ts.eat_word("and") || ts.eat_word("or");
                }
            }
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(subj),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 7. `choose(s) a number [between <v1> and <v2> | <v> or greater] [that hasn't been chosen]` / `choose(s) 1, 2, or 3 [at random]` / `choose(s) a letter`
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
                if ts.next_number(false).is_some()
                    && (ts.eat_phrase(&["or", "greater"]) || ts.eat_phrase(&["or", "less"]))
                {
                    // `0 or greater`
                } else {
                    ts.restore(ncp);
                }
            }
            let _ = ts.eat_phrase(&["that", "hasn't", "been", "chosen"]);
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number),
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["a", "letter"]) {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CardName),
                }],
                (),
            )));
        }
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
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 8. `choose(s) which creatures (attack|block) (this turn|this combat) [and how those creatures block]`
    //    (`Odric, Master Tactician`, `Master Warcraft`, `Melee`, `Berserker's Frenzy`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["which", "creatures"]) && (ts.eat_word("block") || ts.eat_word("attack"))
        {
            let _ = ts.eat_phrase(&["this", "combat"]) || ts.eat_phrase(&["this", "turn"]);
            let _ = ts.eat_phrase(&["and", "how", "those", "creatures", "block"]);
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BinaryMode),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 9. `choose(s) how each player votes this turn` (`Illusion of Choice`)
    if ts.eat_phrase(&["how", "each", "player", "votes", "this", "turn"]) {
        return Some(Ok((
            vec![Effect::PromptChoice {
                chooser: actor,
                kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BinaryMode),
            }],
            (),
        )));
    }

    // 10. `choose(s) and sacrifice(s) <subj>` (`Retribution`)
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["and", "sacrifices"]) || ts.eat_phrase(&["and", "sacrifice"]))
            && let Ok(subject) = try_parse(ts, parse_subject_ref)
        {
            return Some(Ok((
                vec![Effect::ZoneOp {
                    actor,
                    op: ZoneOpKind::Sacrifice,
                    subject,
                    from: vec![ZoneRef {
                        zone: ZoneKind::Battlefield,
                        owner: ZoneOwner::Default,
                    }],
                    to: None,
                    modifiers: EntryModifiers::default(),
                    except: None,
                    count: None,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 11. `choose(s) exactly <N> <flt>` / `choose(s) up to <val> <flt> [from it]` / `choose(s) the same number of <flt>` / `choose(s) a party from among <flt>`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("exactly")
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(SubjectRef::Counted {
                        filter: Box::new(flt),
                        count: CountSpec {
                            count,
                            up_to: false,
                            any_number: false,
                        },
                    }),
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["up", "to"])
            && let Ok(count) = try_parse(ts, parse_value_expr)
            && !ts.peek_phrase(&["target"])
            && !ts.peek_phrase(&["other", "target"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
            && !ts.peek_phrase(&["target"])
        {
            let _ = ts.eat_phrase(&["from", "it"]) || ts.eat_phrase(&["from", "among", "them"]);
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(SubjectRef::Counted {
                        filter: Box::new(flt),
                        count: CountSpec {
                            count,
                            up_to: true,
                            any_number: false,
                        },
                    }),
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["the", "same", "number", "of"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(SubjectRef::Counted {
                        filter: Box::new(flt),
                        count: CountSpec {
                            count: ValueExpr::PriorActionCount(PriorActionKind::ThatMany),
                            up_to: false,
                            any_number: false,
                        },
                    }),
                }],
                (),
            )));
        }
        ts.restore(cp);
        if ts.eat_phrase(&["a", "party", "from", "among"])
            && let Ok(flt) = try_parse(ts, parse_object_filter)
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseObject(SubjectRef::Group(Box::new(flt))),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 12. `choose(s) from [among] <subj|it|them> <obj1> [, <obj2>, and <objN>]`
    //     (`Cataclysm`, `Tragic Arrogance`, `Guided Passage`, `Distended Mindbender`, `Global Ruin`)
    {
        let cp = ts.checkpoint();
        if ts.eat_word("from") {
            let _ = ts.eat_word("among");
            let src_ok = ts.eat_word("it")
                || ts.eat_word("them")
                || try_parse(ts, parse_subject_ref).is_ok();
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
                    return Some(Ok((
                        vec![Effect::PromptChoice {
                            chooser: actor,
                            kind: ChoiceKind::ChooseObject(subj),
                        }],
                        (),
                    )));
                }
            }
        }
        ts.restore(cp);
    }

    // 13. Multi-object list `choose(s) <obj1>, <obj2> [, ...], and <objN> [from among <subj>]`
    //     (`Cataclysmic Gearhulk`, `Ajani`, `Mythos of Snapdax`, `Grenzo's Rebuttal`, `Call to the Void`)
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
                return Some(Ok((
                    vec![Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseObject(SubjectRef::Compound(items)),
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 14. `choose(s) <kw1>, <kw2>, ... or <kwN>` (`Angelic Skirmisher`, `Gabriel Angelfire`)
    {
        let cp = ts.checkpoint();
        if let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            && kws.len() >= 2
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseKeywordAndGrant {
                        subject: SubjectRef::Anaphora(AnaphoraRef::It),
                        keywords: kws,
                        duration: Duration::UntilEndOfTurn,
                    },
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 15. Bare `choose(s) one [at random]` (`Intuition`, `Chandra, Flameshaper`, `Stand or Fall`)
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
                return Some(Ok((
                    vec![Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseObject(SubjectRef::OneOfThem(ValueExpr::Const(1))),
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 16. Named choice modes: `choose(s) <w1> [, <w2>,] or <wN>` (only if reaches EOF / comma / `then`, e.g. `draw step, main phase, or combat phase`)
    {
        let cp = ts.checkpoint();
        let mut opt_count = 0usize;
        let mut saw_or = false;
        while opt_count < 5 {
            let mut words = 0usize;
            while words < 2
                && let Some(w) = ts.peek_word()
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "or" | "and" | "then" | "if" | "unless" | "for" | "until" | "to" | "from"
                )
            {
                ts.next_word();
                words += 1;
            }
            if words == 0 {
                break;
            }
            opt_count += 1;
            let sep_cp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            if ts.eat_word("or") {
                saw_or = true;
                continue;
            }
            if had_comma {
                continue;
            }
            ts.restore(sep_cp);
            break;
        }
        if saw_or
            && opt_count >= 2
            && (ts.is_eof()
                || matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Period | TokenKind::Comma)
                ))
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BinaryMode),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Parse `[secretly] vote(s) for <option1 or option2 | colors | object_filter> [, then those votes are revealed]`.
/// Composes cleanly onto `Effect::PromptChoice`.
pub(super) fn try_parse_vote_verb(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    let _secretly = ts.eat_word("secretly");
    if !(ts.eat_phrase(&["votes", "for"]) || ts.eat_phrase(&["vote", "for"])) {
        ts.restore(cp);
        return None;
    }

    // 1. Color list (`blue, black, red, or green` on `Council Guardian`)
    {
        let ccp = ts.checkpoint();
        let mut colors = ColorMask::EMPTY;
        let mut count = 0usize;
        while let Some(w) = ts.peek_word() {
            if let Some(c) = lookup_color(&w.to_ascii_lowercase()) {
                ts.next_word();
                colors = colors.union(c);
                count += 1;
                let _ = ts.eat_comma();
                let _ = ts.eat_word("or") || ts.eat_word("and");
            } else {
                break;
            }
        }
        if count >= 2 && !colors.is_empty() {
            eat_optional_votes_revealed(ts);
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Color),
                }],
                (),
            )));
        }
        ts.restore(ccp);
    }

    // 2. Subject / object filter (`a player`, `another player`, `up to one creature`, `a nonland permanent you don't control`, `an artifact, creature, or enchantment card in your graveyard`)
    {
        let scp = ts.checkpoint();
        if let Ok(subj) = try_parse(ts, parse_subject_ref) {
            eat_optional_votes_revealed(ts);
            if ts.is_eof()
                || matches!(
                    ts.peek().map(|t| &t.kind),
                    Some(TokenKind::Period | TokenKind::Comma)
                )
            {
                return Some(Ok((
                    vec![Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseObject(subj),
                    }],
                    (),
                )));
            }
        }
        ts.restore(scp);
    }

    // 3. Binary named vote options (`grace or condemnation`, `Redhorn Pass or Mines of Moria`, `planeswalk or chaos`, etc.)
    {
        let ocp = ts.checkpoint();
        let mut lhs_words = 0usize;
        while lhs_words < 4
            && let Some(w) = ts.peek_word()
            && !w.eq_ignore_ascii_case("or")
        {
            ts.next_word();
            lhs_words += 1;
        }
        if lhs_words >= 1 && ts.eat_word("or") {
            let mut rhs_words = 0usize;
            while rhs_words < 4
                && let Some(w) = ts.peek_word()
                && !matches!(
                    w.to_ascii_lowercase().as_str(),
                    "then" | "if" | "for" | "until"
                )
            {
                ts.next_word();
                rhs_words += 1;
            }
            if rhs_words >= 1 {
                eat_optional_votes_revealed(ts);
                if ts.is_eof()
                    || matches!(
                        ts.peek().map(|t| &t.kind),
                        Some(TokenKind::Period | TokenKind::Comma)
                    )
                {
                    return Some(Ok((
                        vec![Effect::PromptChoice {
                            chooser: actor,
                            kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::BinaryMode),
                        }],
                        (),
                    )));
                }
            }
        }
        ts.restore(ocp);
    }

    ts.restore(cp);
    None
}

fn eat_optional_votes_revealed(ts: &mut TokenStream<'_>) {
    let cp = ts.checkpoint();
    if ts.eat_comma()
        && (ts.eat_phrase(&["then", "those", "votes", "are", "revealed"])
            || ts.eat_phrase(&["then", "the", "votes", "are", "revealed"]))
    {
        return;
    }
    ts.restore(cp);
}

/// Extended player-initiated verb frames delegated from `player_verbs.rs` before its final error.
pub(super) fn try_parse_player_verb_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 1. `[secretly] vote(s) for ...`
    if let Some(res) = try_parse_vote_verb(ts, actor) {
        return Some(res);
    }

    // 2. `secretly choose(s) ... [, then (those|the) choices are revealed | , then all players reveal those numbers simultaneously ...]`
    {
        let cp = ts.checkpoint();
        if ts.eat_word("secretly") && (ts.eat_word("choose") || ts.eat_word("chooses")) {
            let chosen = if let Some(res) = try_parse_choose_ext(ts, actor) {
                res.ok()
            } else if let Ok(s) = try_parse(ts, parse_subject_ref) {
                Some((
                    vec![Effect::PromptChoice {
                        chooser: actor,
                        kind: ChoiceKind::ChooseObject(s),
                    }],
                    (),
                ))
            } else {
                None
            };
            if let Some((effs, ())) = chosen {
                let rcp = ts.checkpoint();
                if ts.eat_comma()
                    && (ts.eat_phrase(&["then", "the", "choices", "are", "revealed"])
                        || ts.eat_phrase(&["then", "those", "choices", "are", "revealed"])
                        || ts.eat_phrase(&[
                            "then",
                            "all",
                            "players",
                            "reveal",
                            "those",
                            "numbers",
                            "simultaneously",
                            "and",
                            "determine",
                            "the",
                            "highest",
                            "and",
                            "lowest",
                            "numbers",
                            "revealed",
                            "this",
                            "way",
                        ]))
                {
                    // consumed reveal suffix
                } else {
                    ts.restore(rcp);
                }
                return Some(Ok((effs, ())));
            }
        }
        ts.restore(cp);
    }

    // 3. `meld(s) them into <Proper Name>` (`Hanweir Battlements`, `Titania`, `Graf Rats`, `Gisela`, `Urza`, `Mishra`)
    {
        let cp = ts.checkpoint();
        if (ts.eat_phrase(&["meld", "them", "into"]) || ts.eat_phrase(&["melds", "them", "into"]))
            && eat_proper_name_words(ts)
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject: SubjectRef::Anaphora(AnaphoraRef::Them),
                    mutation: ObjectMutation::StateAction(ObjectStateAction::Transform),
                    duration: Duration::Permanent,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 4. `convert(s) <subj>` (Transformers cards: `Optimus Prime`, `Megatron`, `Jetfire`, `Starscream`, `Ultra Magnus`, `Ratchet`, `Blaster`, `Cyclonus`, `Prowl`, `Blitzwing`, `Arcee`, `Soundwave`)
    if ts.eat_word("convert") || ts.eat_word("converts") {
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(ObjectStateAction::Transform),
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        return Some(Err(ts.err("expected subject after `convert`")));
    }

    // 5. `unattach(es) <subj> [from <host>]` (`Disarm`, `Tamiyo's Compleation`, `Fulgent Distraction`, `Carry Away`, `Ogre Geargrabber`, `Akiri`, `Elbrus`, `Unexpected Request`, `Stolen Uniform`)
    if ts.eat_word("unattach") || ts.eat_word("unattaches") {
        if let Ok(subject) = try_parse(ts, parse_subject_ref) {
            let host = if ts.eat_word("from") {
                try_parse(ts, parse_subject_ref).unwrap_or(SubjectRef::Anaphora(AnaphoraRef::It))
            } else {
                SubjectRef::Anaphora(AnaphoraRef::It)
            };
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::AttachTo(Box::new(host)),
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        return Some(Err(ts.err("expected subject after `unattach`")));
    }

    // 6. `planeswalk(s)` (`Start the TARDIS`, `Path of the Enigma/Animist/Schemer/Ghosthunter/Pyromancer`)
    if ts.eat_word("planeswalk") || ts.eat_word("planeswalks") {
        return Some(Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::VentureIntoDungeon),
                duration: Duration::Instant,
            }],
            (),
        )));
    }

    // 7. `reselect(s) which player [or permanent] <subj> is attacking` / `reselect(s) (its target | the targets) at random [for <subj>]`
    //    (`Portal Mage`, `Windshaper Planetar`, `Misleading Signpost`, `Capricopian`, `Grip of Chaos`, `Chef's Kiss`)
    {
        let cp = ts.checkpoint();
        if ts.eat_word("reselect") || ts.eat_word("reselects") {
            if (ts.eat_phrase(&["which", "player", "or", "permanent"])
                || ts.eat_phrase(&["which", "player"]))
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
                && ts.eat_phrase(&["is", "attacking"])
            {
                return Some(Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::Retarget {
                            subject: Box::new(subj),
                            new_target: None,
                        },
                        duration: Duration::Instant,
                    }],
                    (),
                )));
            }
            ts.restore(cp);
            if (ts.eat_word("reselect") || ts.eat_word("reselects"))
                && (ts.eat_phrase(&["its", "target", "at", "random"])
                    || ts.eat_phrase(&["the", "targets", "at", "random"]))
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
                return Some(Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::Retarget {
                            subject: Box::new(subj),
                            new_target: None,
                        },
                        duration: Duration::Instant,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 8. `activate(s) [the] loyalty abilities of <subj> ...` (`Urza Assembles the Titans`, `Kaito, Dancing Shadow`, `Jace's Machinations`, `The Chain Veil`)
    {
        let cp = ts.checkpoint();
        if ts.eat_word("activate") || ts.eat_word("activates") {
            let _ = ts.eat_word("the");
            if ts.eat_phrase(&["loyalty", "abilities", "of"])
                && try_parse(ts, parse_subject_ref).is_ok()
                && (ts.eat_phrase(&["twice", "this", "turn", "rather", "than", "only", "once"])
                    || ts.eat_phrase(&[
                        "on", "any", "player's", "turn", "any", "time", "you", "could", "cast",
                        "an", "instant",
                    ]))
            {
                return Some(Ok((
                    vec![Effect::ApplyContinuous {
                        effect: Box::new(StaticAbility::GlobalFlag(
                            StaticFlagKind::LoyaltyAsInstant,
                        )),
                        duration: Duration::UntilEndOfTurn,
                    }],
                    (),
                )));
            }
            ts.restore(cp);
            if (ts.eat_word("activate") || ts.eat_word("activates"))
                && ts.eat_phrase(&[
                    "one",
                    "of",
                    "its",
                    "loyalty",
                    "abilities",
                    "once",
                    "this",
                    "turn",
                    "as",
                    "though",
                    "none",
                    "of",
                    "its",
                    "loyalty",
                    "abilities",
                    "have",
                    "been",
                    "activated",
                    "this",
                    "turn",
                ])
            {
                return Some(Ok((
                    vec![Effect::ApplyContinuous {
                        effect: Box::new(StaticAbility::GlobalFlag(
                            StaticFlagKind::LoyaltyAsInstant,
                        )),
                        duration: Duration::UntilEndOfTurn,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 9. `count(s) the number of (<kind> counters on <subj> | <flt>)` (`Touch of the Eternal`, `Chaos Moon`, `Rumbling Ruin`, `Invincible Hymn`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["count", "the", "number", "of"])
            || ts.eat_phrase(&["counts", "the", "number", "of"])
        {
            let expr_opt = {
                let ccp = ts.checkpoint();
                if let Ok(kind) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                    && ts.eat_word("on")
                    && let Ok(subj) = try_parse(ts, parse_subject_ref)
                {
                    Some(ValueExpr::CounterCount {
                        subject: Box::new(subj),
                        kind,
                    })
                } else {
                    ts.restore(ccp);
                    if let Ok(flt) = try_parse(ts, parse_object_filter) {
                        Some(ValueExpr::Count(Box::new(flt)))
                    } else {
                        None
                    }
                }
            };
            if let Some(expr) = expr_opt {
                return Some(Ok((
                    vec![Effect::BindVariable {
                        var: VarName::X,
                        expr,
                        body: Vec::new(),
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 10. `do(es) the same [for <flt/subtypes/kw_list> | with <subj>]` (`Glimpse of Tomorrow`, `Warp World`, `Estrid`, `Grim Captain's Call`, `Queen Kayla`, `The Wedding of River Song`, `Super-Adaptoid`, `Guild Feud`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["do", "the", "same"]) || ts.eat_phrase(&["does", "the", "same"]) {
            if ts.eat_word("for") {
                if try_parse(ts, parse_object_filter).is_ok() {
                    // e.g. `for Aura cards`, `for enchantment cards`, `for artifact or creature cards with mana values 2 and 3`
                    let mv_cp = ts.checkpoint();
                    if ts.eat_phrase(&["with", "mana", "values"])
                        && ts.next_number(false).is_some()
                        && ts.eat_word("and")
                        && ts.next_number(false).is_some()
                    {
                        // consumed `with mana values 2 and 3`
                    } else {
                        ts.restore(mv_cp);
                    }
                } else if try_parse(ts, parse_keyword_list).is_ok() {
                    // e.g. `Super-Adaptoid`: `for flying, first strike, ...`
                } else {
                    // Subtype list: `for Vampire, Dinosaur, and Merfolk`
                    while let Some(w) = ts.peek_word() {
                        if ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some() {
                            ts.next_word();
                            let _ = ts.eat_comma();
                            let _ = ts.eat_word("and") || ts.eat_word("or");
                        } else {
                            break;
                        }
                    }
                }
            } else if ts.eat_word("with") {
                let _ = try_parse(ts, parse_subject_ref);
            }
            return Some(Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::Action(PlayerActionKind::RepeatProcessOnce),
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 11. `spend(s) [<color>] mana as though it were mana of any (color|type) [to ...]`
    //     (`False Dawn`, `Grell Philosopher`, `Xanathar`, `Curse of Hospitality`, `Covetous Urge`, `Arvinox`, `Kotose`)
    {
        let cp = ts.checkpoint();
        if ts.eat_word("spend") || ts.eat_word("spends") {
            if let Some(w) = ts.peek_word()
                && (lookup_color(&w.to_ascii_lowercase()).is_some()
                    || w.eq_ignore_ascii_case("colorless"))
            {
                ts.next_word();
            }
            if ts.eat_phrase(&["mana", "as", "though", "it", "were", "mana", "of", "any"])
                && (ts.eat_word("color") || ts.eat_word("type"))
            {
                if ts.eat_phrase(&["to", "cast"]) {
                    let _ = ts.eat_phrase(&["spells", "this", "way"])
                        || ts.eat_phrase(&["a", "spell", "this", "way"])
                        || ts.eat_phrase(&["that", "spell"])
                        || ts.eat_phrase(&["those", "spells"])
                        || ts.eat_word("it")
                        || ts.eat_word("them");
                } else if ts.eat_phrase(&["to", "activate"]) {
                    let _ = ts.eat_phrase(&["those", "abilities"])
                        || ts.eat_phrase(&["that", "ability"])
                        || ts.eat_word("abilities");
                } else if ts.eat_phrase(&["to", "pay"]) {
                    let _ = ts.eat_phrase(&["that", "spell's", "mana", "cost"])
                        || ts.eat_phrase(&["its", "mana", "cost"]);
                }
                return Some(Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::Action(PlayerActionKind::SpendManaAsAnyType),
                        duration: Duration::UntilEndOfTurn,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 12. Generalized `manifest(s)` (`Ethereal Ambush`, `Jeskai Infiltrator`, `Ghastly Conscription`, `Kozilek`, `Write into Being`, `Omarthis`, `Orochi Soul-Reaver`, `Scroll of Fate`, `Thieving Amalgam`)
    {
        let cp = ts.checkpoint();
        if ts.eat_word("manifest") || ts.eat_word("manifests") {
            if ts.eat_phrase(&["a", "number", "of", "cards", "from", "the", "top", "of"])
                && let Ok(zref) = try_parse(ts, parse_zone_ref)
                && ts.eat_phrase(&["equal", "to"])
                && let Ok(cnt) = try_parse(ts, parse_value_expr)
            {
                return Some(Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject: SubjectRef::Group(Box::new(ObjectFilter {
                            any_card: true,
                            zones: vec![zref],
                            ..Default::default()
                        })),
                        from: vec![zref],
                        to: Some(ZoneDest {
                            zone: ZoneKind::Battlefield,
                            owner: ZoneOwner::Default,
                            library_position: None,
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers {
                            face_down: true,
                            ..EntryModifiers::default()
                        },
                        except: None,
                        count: Some(CountSpec {
                            count: cnt,
                            up_to: false,
                            any_number: false,
                        }),
                    }],
                    (),
                )));
            }
            ts.restore(cp);
            if (ts.eat_word("manifest") || ts.eat_word("manifests"))
                && ts.eat_phrase(&["the", "top"])
            {
                let cnt = if ts.eat_word("card") {
                    Some(ValueExpr::Const(1))
                } else if let Ok(v) = try_parse(ts, parse_value_expr)
                    && (ts.eat_word("cards") || ts.eat_word("card"))
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
                        vec![Effect::ZoneOp {
                            actor,
                            op: ZoneOpKind::Move,
                            subject: SubjectRef::Group(Box::new(ObjectFilter {
                                any_card: true,
                                zones: vec![zref],
                                ..Default::default()
                            })),
                            from: vec![zref],
                            to: Some(ZoneDest {
                                zone: ZoneKind::Battlefield,
                                owner: ZoneOwner::Default,
                                library_position: None,
                                library_order: None,
                                under_control: None,
                            }),
                            modifiers: EntryModifiers {
                                face_down: true,
                                ..EntryModifiers::default()
                            },
                            except: None,
                            count: Some(CountSpec {
                                count,
                                up_to: false,
                                any_number: false,
                            }),
                        }],
                        (),
                    )));
                }
            }
            ts.restore(cp);
            if (ts.eat_word("manifest") || ts.eat_word("manifests"))
                && let Ok(subj) = try_parse(ts, parse_subject_ref)
            {
                let mut from = Vec::new();
                if ts.eat_word("from")
                    && let Ok(zr) = try_parse(ts, parse_zone_ref)
                {
                    from.push(zr);
                }
                return Some(Ok((
                    vec![Effect::ZoneOp {
                        actor,
                        op: ZoneOpKind::Move,
                        subject: subj,
                        from,
                        to: Some(ZoneDest {
                            zone: ZoneKind::Battlefield,
                            owner: ZoneOwner::Default,
                            library_position: None,
                            library_order: None,
                            under_control: None,
                        }),
                        modifiers: EntryModifiers {
                            face_down: true,
                            ..EntryModifiers::default()
                        },
                        except: None,
                        count: None,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    // 13. `note(s) a creature type that hasn't been noted for ~` (`Long List of the Ents`)
    {
        let cp = ts.checkpoint();
        if (ts.eat_word("note") || ts.eat_word("notes"))
            && ts.eat_phrase(&[
                "a", "creature", "type", "that", "hasn't", "been", "noted", "for",
            ])
            && ts.eat_tilde()
        {
            return Some(Ok((
                vec![Effect::PromptChoice {
                    chooser: actor,
                    kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::CreatureType),
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 14. `repeat(s) this process [for ... | until no one ... | until all ...]`
    //     (`Eureka`, `Hypergenesis`, `Plague of Vermin`, `Struggle for Sanity`, `Thieves' Auction`, `Equipoise`, `Kathril`, `Helm of Obedience`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["repeat", "this", "process"])
            || ts.eat_phrase(&["repeats", "this", "process"])
        {
            if ts.eat_phrase(&["any", "number", "of", "times"]) || ts.eat_word("once") {
                // Standard repeat-count suffix (`Ad Nauseam`, etc.)
            } else if ts.eat_word("for") {
                if try_parse(ts, parse_keyword_list).is_ok() {
                    // `Kathril`: `for first strike, double strike, ...`
                } else {
                    while let Ok(_f) = try_parse(ts, parse_object_filter) {
                        let _ = ts.eat_comma();
                        let _ = ts.eat_word("and") || ts.eat_word("or");
                    }
                }
            } else if ts.eat_phrase(&["until", "no", "one"]) {
                let _ = try_parse(ts, |s| parse_player_verb_frame(s, PlayerRef::AnyPlayer));
            } else if ts.eat_phrase(&["until", "all"]) {
                let _ = try_parse(ts, parse_object_filter);
                let _ = ts.eat_phrase(&["have", "been", "chosen"])
                    || ts.eat_phrase(&["have", "been", "exiled"])
                    || ts.eat_phrase(&["have", "been", "discarded"]);
            } else if ts.eat_word("until") {
                // `Helm of Obedience`: `until a creature card or X cards have been put into their graveyard this way, whichever comes first`
                let _ = try_parse(ts, parse_object_filter);
                if ts.eat_word("or") {
                    let _ = opt(ts, parse_value_expr);
                    let _ = ts.eat_word("cards") || ts.eat_word("card");
                }
                let _ = ts.eat_phrase(&["have", "been", "put", "into"])
                    && try_parse(ts, parse_zone_ref).is_ok();
                let _ = ts.eat_phrase(&["this", "way"]);
                let _ = ts.eat_comma();
                let _ = ts.eat_phrase(&["whichever", "comes", "first"]);
            }
            return Some(Ok((
                vec![Effect::MutatePlayer {
                    player: actor,
                    mutation: PlayerMutation::Action(PlayerActionKind::RepeatProcessOnce),
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    None
}

/// Extended `get(s)` player frames (`get an emblem with "..." and "..."`, `get [an additional | that many additional] upkeep step(s) after this step/phase`).
pub(super) fn try_parse_get_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    // 1. `get(s) an additional upkeep step / that many additional upkeep steps after this step/phase`
    //    (`Obeka, Splitter of Seconds`, `The Ninth Doctor`, `Paradox Haze`)
    {
        let cp = ts.checkpoint();
        let count_opt = if ts.eat_phrase(&["an", "additional", "upkeep", "step"]) {
            Some(ValueExpr::Const(1))
        } else if ts.eat_phrase(&["that", "many", "additional", "upkeep", "steps"]) {
            Some(ValueExpr::PriorActionCount(PriorActionKind::ThatMany))
        } else {
            None
        };
        if let Some(cnt) = count_opt
            && (ts.eat_phrase(&["after", "this", "step"])
                || ts.eat_phrase(&["after", "this", "phase"]))
        {
            return Some(Ok((
                vec![Effect::ForEach {
                    over: ForEachDomain::Count(cnt),
                    body: vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::Action(PlayerActionKind::ExtraTurn),
                        duration: Duration::UntilEndOfTurn,
                    }],
                }],
                (),
            )));
        }
        ts.restore(cp);
    }

    // 2. Multi-ability emblem: `get(s) an emblem with "..." and "..."` (`Teferi, Who Slows the Sunset`)
    {
        let cp = ts.checkpoint();
        if ts.eat_phrase(&["an", "emblem", "with"])
            && let Some(q1) = ts.next_quoted()
            && ts.eat_word("and")
            && let Some(q2) = ts.next_quoted()
        {
            let combined = format!("{q1}\n{q2}");
            let emblem_face = compile_face_v2(
                FaceInput {
                    name: "Emblem",
                    type_line: "Emblem",
                    oracle_text: &combined,
                },
                ts.vocab,
            );
            if emblem_face.is_ok() {
                return Some(Ok((
                    vec![Effect::MutatePlayer {
                        player: actor,
                        mutation: PlayerMutation::CreateEmblem(Box::new(emblem_face)),
                        duration: Duration::Permanent,
                    }],
                    (),
                )));
            }
        }
        ts.restore(cp);
    }

    None
}

/// Extended `double(s)` frames: `double your/their life total` (`Revival // Revenge`) and
/// `double the number of each kind of counter you have` (`Aetheric Amplifier`).
pub(super) fn try_parse_double_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["your", "life", "total"])
        || ts.eat_phrase(&["their", "life", "total"])
        || ts.eat_phrase(&["that", "player's", "life", "total"])
    {
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
        && (ts.eat_phrase(&["you", "have"]) || ts.eat_phrase(&["they", "have"]))
    {
        return Some(Ok((
            vec![Effect::MutateObject {
                subject: SubjectRef::Player(actor),
                mutation: ObjectMutation::DoubleCounters { kind: None },
                duration: Duration::Instant,
            }],
            (),
        )));
    }
    ts.restore(cp);
    None
}

/// Extended `have` frames delegated from `player_verbs.rs`:
/// `have two target players exchange life totals` (`Axis of Mortality`),
/// `have <subj> assign its/their combat damage this turn as though it/they weren't blocked` (`Gurzigost`, `Predatory Focus`),
/// `have <subj> become no longer suspected` (`Deadly Complication`),
/// `have <subj> block <subj2>` (`False Orders`).
pub(super) fn try_parse_have_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["two", "target", "players", "exchange", "life", "totals"]) {
        return Some(Ok((
            vec![Effect::AdjustLife {
                player: PlayerRef::TargetPlayer,
                amount: ValueExpr::PlayerStat {
                    player: PlayerRef::TargetPlayer,
                    stat: PlayerStatKind::LifeTotal,
                },
                op: LifeOp::Set,
            }],
            (),
        )));
    }
    if let Ok(subject) = try_parse(ts, parse_subject_ref) {
        if ts.eat_phrase(&[
            "assign", "its", "combat", "damage", "this", "turn", "as", "though", "it", "weren't",
            "blocked",
        ]) || ts.eat_phrase(&[
            "assign", "their", "combat", "damage", "this", "turn", "as", "though", "they",
            "weren't", "blocked",
        ]) {
            return Some(Ok((
                vec![Effect::ApplyContinuous {
                    effect: Box::new(StaticAbility::SubjectRule {
                        subject,
                        rule: SubjectStaticRule::AssignDamageAsUnblocked,
                        condition: None,
                    }),
                    duration: Duration::UntilEndOfTurn,
                }],
                (),
            )));
        }
        if ts.eat_phrase(&["become", "no", "longer", "suspected"])
            || ts.eat_phrase(&["become", "no", "longer", "goaded"])
        {
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::StateAction(ObjectStateAction::Suspect),
                    duration: Duration::Instant,
                }],
                (),
            )));
        }
        if ts.eat_word("block")
            && let Ok(atk) = try_parse(ts, parse_subject_ref)
        {
            let _ = ts.eat_phrase(&["this", "turn", "if", "able"])
                || ts.eat_phrase(&["this", "combat", "if", "able"])
                || ts.eat_phrase(&["if", "able"]);
            return Some(Ok((
                vec![Effect::MutateObject {
                    subject,
                    mutation: ObjectMutation::MustBlockThisTurn(Some(Box::new(atk))),
                    duration: Duration::UntilEndOfCombat,
                }],
                (),
            )));
        }
    }
    ts.restore(cp);
    let _ = actor;
    None
}

/// Extended `roll(s)` helper: supports `roll again` (`Delina`), `four-sided die` (`Arden Angel`),
/// `<val> six-sided dice` (`Attempted Murder`), trailing `and choose one result` (`Endeavor` cycle),
/// and `for each player being attacked and ignore all but the highest roll` (`Iron Mastiff`).
pub(super) fn try_parse_roll_ext(
    ts: &mut TokenStream<'_>,
    actor: PlayerRef,
) -> Option<PResult<(Vec<Effect>, ())>> {
    let cp = ts.checkpoint();
    if ts.eat_word("again") {
        return Some(Ok((
            vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::RollDie {
                    sides: 20,
                    count: 1,
                },
                duration: Duration::Instant,
            }],
            (),
        )));
    }
    let count_expr = if ts.eat_word("a") || ts.eat_word("an") {
        ValueExpr::Const(1)
    } else if let Ok(v) = try_parse(ts, parse_value_expr) {
        v
    } else {
        ts.restore(cp);
        return None;
    };
    let sides = if ts.eat_phrase(&["four-sided", "die"]) || ts.eat_phrase(&["four-sided", "dice"]) {
        Some(4u8)
    } else if ts.eat_phrase(&["six-sided", "die"]) || ts.eat_phrase(&["six-sided", "dice"]) {
        Some(6u8)
    } else if ts.eat_phrase(&["twenty-sided", "die"]) || ts.eat_phrase(&["twenty-sided", "dice"]) {
        Some(20u8)
    } else if let Some(w) = ts.peek_word()
        && let Some(rest) = w.strip_prefix('d').or_else(|| w.strip_prefix('D'))
        && let Ok(s) = rest.parse::<u8>()
    {
        ts.next_word();
        Some(s)
    } else {
        None
    };
    let Some(sides) = sides else {
        ts.restore(cp);
        return None;
    };
    let mut out = if let ValueExpr::Const(c) = count_expr {
        vec![Effect::MutatePlayer {
            player: actor,
            mutation: PlayerMutation::RollDie {
                sides,
                count: u8::try_from(c.max(1)).unwrap_or(1),
            },
            duration: Duration::Instant,
        }]
    } else {
        vec![Effect::ForEach {
            over: ForEachDomain::Count(count_expr),
            body: vec![Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::RollDie { sides, count: 1 },
                duration: Duration::Instant,
            }],
        }]
    };
    let _ = opt(ts, parse_for_each_expr);
    if ts.eat_phrase(&["and", "choose", "one", "result"])
        || ts.eat_phrase(&["and", "ignore", "the", "lower", "roll"])
        || ts.eat_phrase(&["and", "ignore", "all", "but", "the", "highest", "roll"])
    {
        out.push(Effect::PromptChoice {
            chooser: actor,
            kind: ChoiceKind::ChooseCharacteristic(CharacteristicKind::Number),
        });
    }
    Some(Ok((out, ())))
}

/// Extended `from ...` zone/collection tail for `cast` and `play` (`Gix`, `Magus of the Mind`,
/// `Elder Brain`, `Brilliant Ultimatum`, `Bosium Strip`, `Baron Helmut Zemo`, `Gwenom`).
pub(super) fn eat_extended_cast_source(ts: &mut TokenStream<'_>) -> bool {
    let cp = ts.checkpoint();
    if ts.eat_phrase(&["among", "cards", "exiled", "this", "way"])
        || ts.eat_phrase(&["among", "the", "exiled", "cards"])
        || ts.eat_phrase(&["among", "those", "cards"])
        || ts.eat_phrase(&["one", "of", "those", "piles"])
        || ts.eat_phrase(&["the", "other", "pile"])
    {
        return true;
    }
    if ts.eat_phrase(&["the", "top", "of"]) && try_parse(ts, parse_zone_ref).is_ok() {
        return true;
    }
    ts.restore(cp);
    false
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
        let _ = ts.eat_word("you") || ts.eat_word("they");
        let _ = ts.eat_word("may");
        if let Some(Ok((effs, ()))) = try_parse_player_verb_ext(ts, actor) {
            out.extend(effs);
            return;
        }
        ts.restore(cp);
        let _ = ts.eat_comma();
        if ts.eat_phrase(&["and", "mana", "of", "any"])
            && (ts.eat_word("type") || ts.eat_word("color"))
            && ts.eat_phrase(&["can", "be", "spent", "to", "cast"])
            && (ts.eat_phrase(&["a", "spell", "this", "way"])
                || ts.eat_phrase(&["spells", "this", "way"])
                || ts.eat_phrase(&["that", "spell"])
                || ts.eat_phrase(&["those", "spells"])
                || ts.eat_word("it")
                || ts.eat_word("them"))
        {
            out.push(Effect::MutatePlayer {
                player: actor,
                mutation: PlayerMutation::Action(PlayerActionKind::SpendManaAsAnyType),
                duration: dur,
            });
            return;
        }
    }
    ts.restore(cp);
}
