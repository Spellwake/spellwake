//! Strictly-typed compositional parsers for players, keywords, counters, zones,
//! [`ObjectFilter`] (explicit AND `all_types` vs OR `any_types`), and [`SubjectRef`].

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation, StatKind,
    SupertypeMask,
};
use crate::v2::ir::{
    CounterKind, EntryModifiers, FilterFlags, FilterPredicate, LibraryOrder, LibraryPosition,
    ObjectFilter, PlayerRef, PriorActionKind, SharedProperty, StatBound, SubjectRef, ValueExpr,
    ZoneDest, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;
use crate::vocab::{lookup_card_type, lookup_color, lookup_supertype};

/// Parse a typed player reference.
mod subject;

pub use subject::parse_subject_ref;

pub fn parse_player_ref(ts: &mut TokenStream<'_>) -> PResult<PlayerRef> {
    if ts.eat_phrase(&["the", "player", "with", "the", "lowest", "life", "total"]) {
        return Ok(PlayerRef::LowestLifePlayer);
    }
    if ts.eat_phrase(&["the", "player", "with", "the", "highest", "life", "total"])
        || ts.eat_phrase(&[
            "the", "player", "with", "the", "most", "life", "or", "tied", "for", "most", "life",
        ])
        || ts.eat_phrase(&["the", "player", "with", "the", "most", "life"])
    {
        return Ok(PlayerRef::HighestLifePlayer);
    }
    if ts.eat_phrase(&[
        "each", "player", "who", "searched", "their", "library", "this", "way",
    ]) {
        return Ok(PlayerRef::SearchedLibraryPlayer);
    }
    if ts.eat_phrase(&["each", "player", "who", "controls", "the", "most"])
        || ts.eat_phrase(&["the", "player", "who", "controls", "the", "most"])
    {
        let _ = try_parse(ts, parse_object_filter);
        return Ok(PlayerRef::MostObjectsPlayer);
    }
    if ts.eat_phrase(&["each", "other", "opponent"]) {
        return Ok(PlayerRef::EachOtherOpponent);
    }
    if ts.eat_phrase(&["each", "other", "player"]) {
        return Ok(PlayerRef::EachOtherPlayer);
    }
    if ts.eat_phrase(&["each", "opponent"]) || ts.eat_phrase(&["your", "opponents"]) {
        return Ok(PlayerRef::EachOpponent);
    }
    if ts.eat_phrase(&["each", "player"])
        || ts.eat_phrase(&["all", "players"])
        || ts.eat_word("players")
    {
        return Ok(PlayerRef::EachPlayer);
    }
    if ts.eat_phrase(&[
        "that",
        "player",
        "or",
        "that",
        "planeswalker's",
        "controller",
    ]) || ts.eat_phrase(&["that", "player", "or", "that", "permanent's", "controller"])
        || ts.eat_phrase(&[
            "target",
            "player",
            "or",
            "that",
            "planeswalker's",
            "controller",
        ])
        || ts.eat_phrase(&[
            "target",
            "player",
            "or",
            "that",
            "permanent's",
            "controller",
        ])
    {
        return Ok(PlayerRef::ThatPlayer);
    }
    if ts.eat_phrase(&["target", "opponent"]) {
        return Ok(PlayerRef::TargetOpponent);
    }
    if ts.eat_phrase(&["target", "player"]) {
        return Ok(PlayerRef::TargetPlayer);
    }
    if ts.eat_phrase(&["an", "opponent"])
        || ts.eat_phrase(&["any", "opponent"])
        || ts.eat_phrase(&["one", "of", "your", "opponents"])
    {
        return Ok(PlayerRef::AnyOpponent);
    }
    if ts.eat_phrase(&["a", "player"]) || ts.eat_phrase(&["any", "player"]) {
        return Ok(PlayerRef::AnyPlayer);
    }
    if ts.eat_phrase(&["that", "attacking", "player"])
        || ts.eat_phrase(&["that", "player"])
        || ts.eat_phrase(&["the", "player"])
        || ts.eat_word("they")
    {
        return Ok(PlayerRef::ThatPlayer);
    }
    if ts.eat_phrase(&["defending", "player"]) {
        return Ok(PlayerRef::DefendingPlayer);
    }
    if ts.eat_phrase(&["active", "player"]) || ts.eat_phrase(&["the", "attacking", "player"]) {
        return Ok(PlayerRef::ActivePlayer);
    }
    if ts.eat_phrase(&["enchanted", "player"]) || ts.eat_phrase(&["enchanted", "opponent"]) {
        return Ok(PlayerRef::EnchantedPlayer);
    }
    if ts.eat_phrase(&["that", "creature's", "controller"])
        || ts.eat_phrase(&["that", "permanent's", "controller"])
        || ts.eat_phrase(&["that", "land's", "controller"])
        || ts.eat_phrase(&["that", "artifact's", "controller"])
        || ts.eat_phrase(&["that", "source's", "controller"])
        || ts.eat_phrase(&["that", "source's", "owner"])
        || ts.eat_phrase(&["that", "card's", "owner"])
        || ts.eat_phrase(&["that", "creature's", "owner"])
        || ts.eat_phrase(&["target", "creature's", "controller"])
        || ts.eat_phrase(&["target", "creature's", "owner"])
        || ts.eat_phrase(&["target", "permanent's", "owner"])
        || ts.eat_phrase(&["target", "nonland", "permanent's", "owner"])
        || ts.eat_phrase(&["target", "spell's", "controller"])
    {
        return Ok(PlayerRef::ItsController);
    }
    {
        let ocp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_word("'s") && ts.eat_word("owner") {
            return Ok(PlayerRef::ItsOwner);
        }
        ts.restore(ocp);
    }
    if ts.eat_phrase(&["that", "spell's", "controller"]) {
        return Ok(PlayerRef::ThatSpellController);
    }
    if ts.eat_phrase(&["its", "controller"]) {
        return Ok(PlayerRef::ItsController);
    }
    if ts.eat_phrase(&["the", "owner", "of"]) {
        let _ = try_parse(ts, parse_subject_ref);
        return Ok(PlayerRef::ItsOwner);
    }
    if ts.eat_phrase(&["its", "owner"]) {
        return Ok(PlayerRef::ItsOwner);
    }
    if ts.eat_phrase(&["the", "chosen", "player"]) {
        return Ok(PlayerRef::ChosenPlayer);
    }
    if ts.eat_word("you") {
        return Ok(PlayerRef::You);
    }
    Err(ts.err("expected player reference"))
}

pub use crate::v2::keywords::{parse_keyword_kind, parse_keyword_list, parse_keyword_spec};

/// Parse a typed [`CounterKind`] (strictly rejects articles `"a"`/`"an"` as counter kinds!).
pub fn parse_counter_kind(ts: &mut TokenStream<'_>) -> PResult<CounterKind> {
    if let Some((dx, dy)) = ts.next_pt_mod() {
        return match (dx, dy) {
            (crate::v2::ir::PtDelta::Fixed(1), crate::v2::ir::PtDelta::Fixed(1)) => {
                Ok(CounterKind::PlusOnePlusOne)
            }
            (crate::v2::ir::PtDelta::Fixed(-1), crate::v2::ir::PtDelta::Fixed(-1)) => {
                Ok(CounterKind::MinusOneMinusOne)
            }
            (crate::v2::ir::PtDelta::Fixed(p), crate::v2::ir::PtDelta::Fixed(t)) => Ok(
                CounterKind::Pt(i8::try_from(p).unwrap_or(0), i8::try_from(t).unwrap_or(0)),
            ),
            _ => Err(ts.err("unsupported P/T counter kind")),
        };
    }
    let cp = ts.checkpoint();
    if let Ok(kw) = try_parse(ts, parse_keyword_kind) {
        return Ok(CounterKind::StunOrKeyword(kw));
    }
    let Some(w) = ts.next_word() else {
        return Err(ts.err("expected counter kind"));
    };
    let low = w.to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "a" | "an" | "the" | "counter" | "counters" | "on" | "from" | "of"
    ) {
        ts.restore(cp);
        return Err(ts.err("article or preposition cannot be a counter kind"));
    }
    let kind = match low.as_str() {
        "loyalty" => CounterKind::Loyalty,
        "time" => CounterKind::Time,
        "energy" => CounterKind::Energy,
        "poison" => CounterKind::Poison,
        "experience" => CounterKind::Experience,
        "finality" => CounterKind::Finality,
        "stun" => CounterKind::Stun,
        "charge" => CounterKind::Charge,
        "spore" => CounterKind::Spore,
        "slumber" => CounterKind::Slumber,
        "lore" => CounterKind::Lore,
        "shield" => CounterKind::Shield,
        "oil" => CounterKind::Oil,
        other => CounterKind::Named(other.to_string()),
    };
    Ok(kind)
}

/// Parse a typed [`ZoneRef`] (e.g. `your graveyard`, `exile`, `the command zone`, `all hands`).
pub fn parse_zone_ref(ts: &mut TokenStream<'_>) -> PResult<ZoneRef> {
    if ts.eat_word("exile") {
        return Ok(ZoneRef {
            zone: ZoneKind::Exile,
            owner: ZoneOwner::Default,
        });
    }
    if ts.eat_phrase(&["the", "battlefield"]) || ts.eat_word("battlefield") {
        return Ok(ZoneRef {
            zone: ZoneKind::Battlefield,
            owner: ZoneOwner::Default,
        });
    }
    if ts.eat_phrase(&["the", "command", "zone"]) || ts.eat_phrase(&["command", "zone"]) {
        return Ok(ZoneRef {
            zone: ZoneKind::CommandZone,
            owner: ZoneOwner::Default,
        });
    }
    if ts.eat_phrase(&["the", "stack"]) || ts.eat_word("stack") {
        return Ok(ZoneRef {
            zone: ZoneKind::Stack,
            owner: ZoneOwner::Default,
        });
    }
    if ts.eat_phrase(&["outside", "the", "game"]) {
        return Ok(ZoneRef {
            zone: ZoneKind::OutsideTheGame,
            owner: ZoneOwner::Default,
        });
    }

    let cp = ts.checkpoint();
    // Longest-prefix match FIRST so `your opponents'` isn't shadowed by `your`
    // and `their owners'` / `their owner's` isn't shadowed by `their`!
    let owner = if ts.eat_phrase(&["your", "opponents'"]) || ts.eat_word("opponents'") {
        ZoneOwner::Opponents
    } else if ts.eat_phrase(&["their", "owners'"]) || ts.eat_phrase(&["their", "owner's"]) {
        ZoneOwner::TheirOwners
    } else if ts.eat_phrase(&["that", "creature's", "owner's"])
        || ts.eat_phrase(&["that", "permanent's", "owner's"])
        || ts.eat_phrase(&["that", "card's", "owner's"])
        || ts.eat_phrase(&["its", "owner's"])
        || ts.eat_phrase(&["its", "controller's"])
        || ts.eat_phrase(&["that", "creature's", "controller's"])
        || ts.eat_phrase(&["that", "spell's", "controller's"])
    {
        ZoneOwner::ItsOwner
    } else if ts.eat_phrase(&["target", "opponent's"]) {
        ZoneOwner::TargetOpponent
    } else if ts.eat_phrase(&["target", "player's"]) {
        ZoneOwner::TargetPlayer
    } else if ts.eat_phrase(&["that", "player's"]) || ts.eat_word("their") {
        ZoneOwner::ThatPlayer
    } else if ts.eat_phrase(&["an", "opponent's"]) || ts.eat_phrase(&["each", "opponent's"]) {
        ZoneOwner::Opponent
    } else if ts.eat_phrase(&["each", "player's"]) {
        ZoneOwner::EachPlayer
    } else if ts.eat_word("your") {
        ZoneOwner::You
    } else if ts.eat_word("all") {
        ZoneOwner::All
    } else if ts.eat_phrase(&["a", "single"])
        || ts.eat_word("any")
        || ts.eat_word("a")
        || ts.eat_word("that")
    {
        ZoneOwner::Any
    } else {
        ZoneOwner::Default
    };

    let zone = if ts.eat_word("graveyard") || ts.eat_word("graveyards") {
        ZoneKind::Graveyard
    } else if ts.eat_word("hand") || ts.eat_word("hands") {
        ZoneKind::Hand
    } else if ts.eat_word("library") || ts.eat_word("libraries") {
        ZoneKind::Library
    } else {
        ts.restore(cp);
        return Err(ts.err("expected zone name"));
    };
    Ok(ZoneRef { zone, owner })
}

/// Parse a typed zone destination (`into your hand`, `onto the battlefield tapped under their owners' control`,
/// `on the bottom of your library in a random order`, etc.) along with entry modifiers.
pub fn parse_zone_dest(ts: &mut TokenStream<'_>) -> PResult<(ZoneDest, EntryModifiers)> {
    let cp = ts.checkpoint();
    // Optional preposition: to / into / onto / on
    let _ = ts.eat_word("to") || ts.eat_word("into") || ts.eat_word("onto") || ts.eat_word("on");

    let mut modifiers = EntryModifiers::default();

    // Library position destinations: `their/its owner's choice of the top or bottom of <lib>`, `the top of <lib>`, `the bottom of <lib>`
    if (ts.eat_phrase(&["their", "choice", "of", "the", "top", "or", "bottom", "of"])
        || ts.eat_phrase(&["your", "choice", "of", "the", "top", "or", "bottom", "of"])
        || ts.eat_phrase(&["the", "top", "or", "bottom", "of"]))
        && let Ok(zref) = try_parse(ts, parse_zone_ref)
        && zref.zone == ZoneKind::Library
    {
        return Ok((
            ZoneDest {
                zone: ZoneKind::Library,
                owner: zref.owner,
                library_position: Some(LibraryPosition::TopOrBottom),
                library_order: None,
                under_control: None,
            },
            modifiers,
        ));
    }
    if ts.eat_phrase(&["the", "top", "of"]) || ts.eat_phrase(&["top", "of"]) {
        let zref = parse_zone_ref(ts)?;
        if zref.zone != ZoneKind::Library {
            ts.restore(cp);
            return Err(ts.err("expected library after `top of`"));
        }
        if ts.eat_phrase(&["or", "on", "the", "bottom"]) || ts.eat_phrase(&["or", "bottom"]) {
            return Ok((
                ZoneDest {
                    zone: ZoneKind::Library,
                    owner: zref.owner,
                    library_position: Some(LibraryPosition::TopOrBottom),
                    library_order: None,
                    under_control: None,
                },
                modifiers,
            ));
        }
        let order = parse_library_order(ts);
        return Ok((
            ZoneDest {
                zone: ZoneKind::Library,
                owner: zref.owner,
                library_position: Some(LibraryPosition::Top),
                library_order: order,
                under_control: None,
            },
            modifiers,
        ));
    }
    if ts.eat_phrase(&["the", "bottom", "of"]) || ts.eat_phrase(&["bottom", "of"]) {
        let zref = parse_zone_ref(ts)?;
        if zref.zone != ZoneKind::Library {
            ts.restore(cp);
            return Err(ts.err("expected library after `bottom of`"));
        }
        let order = parse_library_order(ts);
        return Ok((
            ZoneDest {
                zone: ZoneKind::Library,
                owner: zref.owner,
                library_position: Some(LibraryPosition::Bottom),
                library_order: order,
                under_control: None,
            },
            modifiers,
        ));
    }

    let zref = parse_zone_ref(ts)?;
    let mut under_control = None;
    let mut library_position = None;
    if zref.zone == ZoneKind::Library {
        if ts.eat_phrase(&["second", "from", "the", "top"]) {
            library_position = Some(LibraryPosition::FromTop(2));
        } else if ts.eat_phrase(&["third", "from", "the", "top"]) {
            library_position = Some(LibraryPosition::FromTop(3));
        } else if ts.eat_phrase(&["fourth", "from", "the", "top"]) {
            library_position = Some(LibraryPosition::FromTop(4));
        }
    }

    // Parse trailing battlefield/exile modifiers in any order
    loop {
        if ts.eat_word("tapped") {
            modifiers.tapped = true;
            continue;
        }
        if ts.eat_phrase(&["and", "attacking"]) || ts.eat_word("attacking") {
            modifiers.attacking = true;
            continue;
        }
        if ts.eat_phrase(&["face", "down"]) || ts.eat_word("face-down") {
            modifiers.face_down = true;
            continue;
        }
        if ts.eat_phrase(&["attached", "to"])
            && let Ok(host) = try_parse(ts, parse_subject_ref)
        {
            modifiers.attached_to = Some(Box::new(host));
            continue;
        }
        if ts.eat_word("transformed") {
            modifiers.transformed = true;
            continue;
        }
        if ts.eat_phrase(&["under", "your", "control"]) {
            under_control = Some(ZoneOwner::You);
            continue;
        }
        if ts.eat_phrase(&["under", "their", "owners'", "control"])
            || ts.eat_phrase(&["under", "their", "owner's", "control"])
        {
            under_control = Some(ZoneOwner::TheirOwners);
            continue;
        }
        if ts.eat_phrase(&["under", "its", "owner's", "control"]) {
            under_control = Some(ZoneOwner::ItsOwner);
            continue;
        }
        if ts.eat_phrase(&[
            "under",
            "the",
            "control",
            "of",
            "that",
            "creature's",
            "owner",
        ]) {
            under_control = Some(ZoneOwner::ThatCreatureOwner);
            continue;
        }
        if ts.eat_phrase(&["under", "that", "player's", "control"]) {
            under_control = Some(ZoneOwner::ThatPlayer);
            continue;
        }
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let wcp = ts.checkpoint();
            ts.eat_word("with");
            let cnt = ts.next_number(true).unwrap_or(1);
            let _ = ts.eat_word("additional");
            if let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                modifiers.with_counter = Some((ck, u16::try_from(cnt.max(1)).unwrap_or(1)));
                continue;
            }
            ts.restore(wcp);
        }
        break;
    }

    Ok((
        ZoneDest {
            zone: zref.zone,
            owner: zref.owner,
            library_position,
            library_order: None,
            under_control,
        },
        modifiers,
    ))
}

fn parse_library_order(ts: &mut TokenStream<'_>) -> Option<LibraryOrder> {
    if ts.eat_phrase(&["in", "a", "random", "order"]) || ts.eat_phrase(&["in", "random", "order"]) {
        Some(LibraryOrder::RandomOrder)
    } else if ts.eat_phrase(&["in", "any", "order"]) {
        Some(LibraryOrder::AnyOrder)
    } else {
        None
    }
}

/// Strictly-typed noun-phrase [`ObjectFilter`] parser with explicit `all_types` (AND) vs
/// `any_types` (OR), typed stat bounds, `"power or toughness"` disjunction, `"differently named"`,
/// controller/owner, multi-zone sources, and relational relative clauses.
pub fn parse_object_filter(ts: &mut TokenStream<'_>) -> PResult<ObjectFilter> {
    let start_cp = ts.checkpoint();
    let mut flt = ObjectFilter::default();

    // Optional leading articles / `another` / `other` / `differently named`
    let _ = ts.eat_word("a") || ts.eat_word("an") || ts.eat_word("the");
    // Support self-named planeswalker subtype filter (`a ~ planeswalker` normalized from `a Lukka planeswalker`)
    {
        let pcp = ts.checkpoint();
        if ts.eat_tilde() && (ts.eat_word("planeswalker") || ts.eat_word("planeswalkers")) {
            flt.any_types = flt.any_types.union(CardTypeMask::PLANESWALKER);
            flt.predicates.push(FilterPredicate::NamedSelf);
            parse_filter_post_modifiers(ts, &mut flt)?;
            return Ok(flt);
        }
        ts.restore(pcp);
    }
    if ts.eat_word("other") || ts.eat_word("another") {
        flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
    }
    if ts.eat_phrase(&["differently", "named"]) {
        flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
    }

    // Track card types + subtypes with explicit AND vs OR conjunction semantics
    let mut card_types_seen: Vec<CardTypeMask> = Vec::new();
    let mut saw_or_between_types = false;
    let mut pending_or_type = false;
    let mut saw_subtype = false;
    let mut saw_or_between_subtypes = false;
    let mut pending_or_subtype = false;

    loop {
        let cp = ts.checkpoint();
        // Handle `differently named` anywhere in modifier list
        if ts.eat_phrase(&["differently", "named"]) {
            flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
            continue;
        }
        // Handle comma between pre-nominal modifiers like `nonwhite, nonartifact` or `nonartifact, nonred`.
        // NEVER eat a comma once a head noun/type/subtype has been parsed (e.g. `three or more enchantments, Angels ...`).
        if ts.eat_comma() {
            let has_head = flt.any_permanent
                || flt.any_card
                || flt.any_spell
                || flt.is_ability
                || !card_types_seen.is_empty()
                || saw_subtype;
            if !has_head
                && let Some(w) = ts.peek_word()
                && is_filter_modifier_word(w, ts.vocab)
            {
                continue;
            }
            // Allow Oxford comma inside card-type + subtype lists (`artifact, creature, or enchantment`, `creature, Spacecraft, or Planet`)
            if !card_types_seen.is_empty() {
                if ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("or") || w.eq_ignore_ascii_case("and"))
                    && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if lookup_card_type(&nw.to_ascii_lowercase()).is_some() || ts.vocab.lookup_subtype(&nw.to_ascii_lowercase()).is_some())
                    })
                {
                    pending_or_type = true;
                    continue;
                }
                if ts.peek_word().is_some_and(|w| {
                    lookup_card_type(&w.to_ascii_lowercase()).is_some()
                        || ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some()
                }) && matches!(ts.peek_n(1).map(|t| &t.kind), Some(TokenKind::Comma))
                {
                    pending_or_type = true;
                    continue;
                }
            }
            // Allow Oxford comma inside subtype lists (`Elf, Warrior, or Tyvar`, `Cleric, Rogue, Warrior, and Wizard`)
            if saw_subtype {
                if ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("or") || w.eq_ignore_ascii_case("and"))
                    && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if ts.vocab.lookup_subtype(&nw.to_ascii_lowercase()).is_some())
                    })
                {
                    pending_or_subtype = true;
                    continue;
                }
                if ts
                    .peek_word()
                    .is_some_and(|w| lookup_card_type(&w.to_ascii_lowercase()).is_some())
                    && matches!(
                        ts.peek_n(1).map(|t| &t.kind),
                        Some(TokenKind::Comma)
                            | Some(TokenKind::Word("and"))
                            | Some(TokenKind::Word("or"))
                    )
                {
                    pending_or_type = true;
                    continue;
                }
                if ts
                    .peek_word()
                    .is_some_and(|w| ts.vocab.lookup_subtype(&w.to_ascii_lowercase()).is_some())
                    && matches!(ts.peek_n(1).map(|t| &t.kind), Some(TokenKind::Comma))
                {
                    pending_or_subtype = true;
                    continue;
                }
            }
            ts.restore(cp);
            break;
        }
        // `<P>/<T>` adjective before noun (`Target 1/1 creature`, `target 2/2 artifact creature`)
        if let Some(TokenKind::BasePt(p, t)) = ts.peek().map(|tok| &tok.kind)
            && !flt.any_permanent
            && !flt.any_card
            && !flt.any_spell
            && card_types_seen.is_empty()
        {
            let (p, t) = (*p, *t);
            ts.pos += 1;
            flt.stat_bounds.push(StatBound {
                stat: StatKind::Power,
                op: CompareOp::Eq,
                value: Box::new(ValueExpr::Const(p)),
            });
            flt.stat_bounds.push(StatBound {
                stat: StatKind::Toughness,
                op: CompareOp::Eq,
                value: Box::new(ValueExpr::Const(t)),
            });
            continue;
        }
        let Some(word) = ts.peek_word() else {
            break;
        };
        let low = word.to_ascii_lowercase();

        // Conjunctions inside noun phrase (`blue or red`, `artifact or creature`, `spell or ability`, `attacking or blocking`)
        if matches!(low.as_str(), "or" | "and/or") {
            let next_ok = ts.peek_n(1).is_some_and(|t| match t.kind {
                TokenKind::Word(nw) => {
                    let nl = nw.to_ascii_lowercase();
                    lookup_color(&nl).is_some()
                        || lookup_card_type(&nl).is_some()
                        || ts.vocab.lookup_subtype(&nl).is_some()
                        || matches!(
                            nl.as_str(),
                            "a" | "an"
                                | "activated"
                                | "triggered"
                                | "ability"
                                | "abilities"
                                | "blocking"
                                | "attacking"
                                | "permanent"
                                | "permanents"
                                | "card"
                                | "cards"
                                | "token"
                                | "tokens"
                                | "spell"
                                | "spells"
                                | "goaded"
                                | "suspected"
                                | "warped"
                                | "suspended"
                                | "kicked"
                        )
                }
                _ => false,
            });
            if next_ok {
                // Guard against eating `or a <noun> <verb>` where `or` joins two condition clauses
                // (e.g. `you control a land creature or a land entered the battlefield under your control this turn`)!
                let is_cond_clause = matches!(
                    ts.peek_n(1).map(|t| &t.kind),
                    Some(TokenKind::Word(w)) if w.eq_ignore_ascii_case("a") || w.eq_ignore_ascii_case("an")
                ) && ts.peek_n(3).is_some_and(|t| {
                    matches!(
                        t.kind,
                        TokenKind::Word(v)
                            if matches!(
                                v.to_ascii_lowercase().as_str(),
                                "entered"
                                    | "died"
                                    | "left"
                                    | "was"
                                    | "were"
                                    | "is"
                                    | "are"
                                    | "has"
                                    | "have"
                                    | "attacked"
                                    | "cast"
                            )
                    )
                });
                if is_cond_clause {
                    break;
                }
                ts.next_word();
                let _ = ts.eat_word("a") || ts.eat_word("an");
                if !card_types_seen.is_empty() {
                    pending_or_type = true;
                }
                if saw_subtype {
                    pending_or_subtype = true;
                }
                continue;
            }
            break;
        }
        if low == "and" {
            // Only consume `and` inside a noun phrase if immediately followed by a color, card type
            // ("instant and sorcery spells"), or subtype ("Vampires and Zombies"), NOT a verb!
            let next_ok = ts.peek_n(1).is_some_and(|t| match t.kind {
                TokenKind::Word(nw) => {
                    let nl = nw.to_ascii_lowercase();
                    lookup_color(&nl).is_some()
                        || lookup_card_type(&nl).is_some()
                        || ts.vocab.lookup_subtype(&nl).is_some()
                }
                _ => false,
            });
            // Guard against `"spells and ..."` where `and` joins two clauses/objects rather than two adjectives
            let already_has_head =
                flt.any_permanent || flt.any_card || flt.any_spell || flt.is_ability;
            if next_ok && !already_has_head {
                ts.next_word();
                if !card_types_seen.is_empty() {
                    pending_or_type = true;
                }
                if saw_subtype {
                    pending_or_subtype = true;
                }
                continue;
            }
            break;
        }

        // Core noun & adjective tokens
        match low.as_str() {
            "other" => {
                // Leave `other than <subj>` for `parse_filter_post_modifiers`!
                if ts.peek_n(1).is_some_and(
                    |t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("than")),
                ) {
                    break;
                }
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            }
            "another" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            }
            "kicked" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::KICKED);
            }
            "goaded" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::GOADED);
            }
            "suspected" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::SUSPECTED);
            }
            "warped" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::WARPED);
            }
            "suspended" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::SUSPENDED);
            }
            "stickered" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::STICKERED);
            }
            "renowned" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::RENOWNED);
            }
            "adventure" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::HAS_ADVENTURE);
            }
            "permanent" | "permanents" => {
                ts.next_word();
                flt.any_permanent = true;
            }
            "card" | "cards" => {
                ts.next_word();
                flt.any_card = true;
            }
            "spell" | "spells" => {
                ts.next_word();
                flt.any_spell = true;
            }
            "ability" | "abilities" => {
                ts.next_word();
                flt.is_ability = true;
            }
            "activated" | "triggered" => {
                ts.next_word();
                flt.is_ability = true;
            }
            "token" | "tokens" => {
                ts.next_word();
                flt.token = Some(true);
            }
            "nontoken" => {
                ts.next_word();
                flt.token = Some(false);
            }
            "colorless" => {
                ts.next_word();
                flt.color_arity = Some(ColorArity::Colorless);
            }
            "colored" => {
                ts.next_word();
                flt.color_arity = Some(ColorArity::Colored);
            }
            "multicolored" => {
                ts.next_word();
                flt.color_arity = Some(ColorArity::Multicolored);
            }
            "monocolored" => {
                ts.next_word();
                flt.color_arity = Some(ColorArity::Monocolored);
            }
            "tapped" => {
                // Leave `tapped this way` for `parse_filter_post_modifiers`!
                if ts.peek_phrase(&["tapped", "this", "way"]) {
                    break;
                }
                ts.next_word();
                flt.tapped = Some(true);
            }
            "untapped" | "nontapped" => {
                ts.next_word();
                flt.tapped = Some(false);
            }
            "attacking" => {
                // Leave post-nominal `attacking <player>` for `parse_filter_post_modifiers`!
                let has_head = flt.any_permanent
                    || flt.any_card
                    || flt.any_spell
                    || flt.is_ability
                    || !card_types_seen.is_empty()
                    || saw_subtype;
                if has_head {
                    break;
                }
                ts.next_word();
                flt.combat = Some(if flt.combat == Some(CombatState::Blocking) {
                    CombatState::AttackingOrBlocking
                } else {
                    CombatState::Attacking
                });
            }
            "blocking" => {
                // Leave post-nominal `blocking <subj>` / `blocking or blocked by <subj>` for `parse_filter_post_modifiers`!
                let has_head = flt.any_permanent
                    || flt.any_card
                    || flt.any_spell
                    || flt.is_ability
                    || !card_types_seen.is_empty()
                    || saw_subtype;
                if has_head {
                    break;
                }
                ts.next_word();
                flt.combat = Some(if flt.combat == Some(CombatState::Attacking) {
                    CombatState::AttackingOrBlocking
                } else {
                    CombatState::Blocking
                });
            }
            "blocked" => {
                let has_head = flt.any_permanent
                    || flt.any_card
                    || flt.any_spell
                    || flt.is_ability
                    || !card_types_seen.is_empty()
                    || saw_subtype;
                if has_head {
                    break;
                }
                ts.next_word();
                flt.combat = Some(CombatState::Blocked);
            }
            "unblocked" => {
                ts.next_word();
                flt.combat = Some(CombatState::Unblocked);
            }
            "modified" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::MODIFIED);
            }
            "transformed" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::TRANSFORMED);
            }
            "equipped" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::EQUIPPED);
            }
            "enchanted" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
            }
            "face-down" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::FACE_DOWN);
            }
            "face-up" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::FACE_UP);
            }
            "commander" | "commanders" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::IS_COMMANDER);
            }
            "noncommander" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::NOT_COMMANDER);
            }
            "nonland" => {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND);
            }
            "noncreature" => {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::CREATURE);
            }
            "nonartifact" => {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::ARTIFACT);
            }
            "nonenchantment" => {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::ENCHANTMENT);
            }
            "nonplaneswalker" => {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::PLANESWALKER);
            }
            "nonbasic" => {
                ts.next_word();
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::BASIC);
            }
            "nonlegendary" => {
                ts.next_word();
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
            }
            "nonwhite" => {
                ts.next_word();
                flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::WHITE);
            }
            "nonblue" => {
                ts.next_word();
                flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLUE);
            }
            "nonblack" => {
                ts.next_word();
                flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::BLACK);
            }
            "nonred" => {
                ts.next_word();
                flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::RED);
            }
            "nongreen" => {
                ts.next_word();
                flt.forbidden_colors = flt.forbidden_colors.union(ColorMask::GREEN);
            }
            _ => {
                if let Some(c) = lookup_color(&low) {
                    ts.next_word();
                    flt.any_colors = flt.any_colors.union(c);
                } else if let Some(st) = lookup_supertype(&low) {
                    ts.next_word();
                    flt.required_supertypes = flt.required_supertypes.union(st);
                } else if let Some(ct) = lookup_card_type(&low) {
                    ts.next_word();
                    if pending_or_type {
                        saw_or_between_types = true;
                        pending_or_type = false;
                    }
                    card_types_seen.push(ct);
                } else if let Some(rest) = low.strip_prefix("non-")
                    && let Some(sub) = ts.vocab.lookup_subtype(rest)
                {
                    ts.next_word();
                    flt.forbidden_subtypes.push(sub.to_string());
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    if pending_or_subtype {
                        saw_or_between_subtypes = true;
                        pending_or_subtype = false;
                    }
                    saw_subtype = true;
                    flt.subtypes.push(sub.to_string());
                } else {
                    break;
                }
            }
        }
    }

    // Finalize explicit `all_types` (AND) vs `any_types` (OR)
    if card_types_seen.len() >= 2 && !saw_or_between_types {
        for ct in card_types_seen {
            flt.all_types = flt.all_types.union(ct);
        }
    } else {
        for ct in card_types_seen {
            flt.any_types = flt.any_types.union(ct);
        }
    }
    if saw_or_between_subtypes && flt.subtypes.len() >= 2 {
        flt.subtypes_match_any = true;
    }

    if flt.is_empty() {
        ts.restore(start_cp);
        return Err(ts.err("expected object filter noun phrase"));
    }

    // Parse trailing post-nominal modifiers in any order (controller, owner, zone, stat bounds,
    // counter predicates, keywords, relative clauses).
    parse_filter_post_modifiers(ts, &mut flt)?;

    Ok(flt)
}

fn is_filter_modifier_word(w: &str, vocab: &crate::vocab::Vocabulary) -> bool {
    let low = w.to_ascii_lowercase();
    lookup_color(&low).is_some()
        || lookup_card_type(&low).is_some()
        || lookup_supertype(&low).is_some()
        || vocab.lookup_subtype(&low).is_some()
        || matches!(
            low.as_str(),
            "nonwhite"
                | "nonblue"
                | "nonblack"
                | "nonred"
                | "nongreen"
                | "nonland"
                | "noncreature"
                | "nonartifact"
                | "nonenchantment"
                | "nonplaneswalker"
                | "nonbasic"
                | "nonlegendary"
                | "nontoken"
                | "tapped"
                | "untapped"
                | "attacking"
                | "blocking"
                | "modified"
        )
}

fn parse_filter_post_modifiers(ts: &mut TokenStream<'_>, flt: &mut ObjectFilter) -> PResult<()> {
    loop {
        // Controller / owner clauses
        if ts.eat_phrase(&["you", "don't", "control"]) {
            flt.controller = Some(ControllerRelation::NotYou);
            continue;
        }
        if ts.eat_phrase(&["defending", "player", "controls"]) {
            flt.controller = Some(ControllerRelation::DefendingPlayer);
            continue;
        }
        if ts.eat_phrase(&["on", "the", "battlefield"]) {
            flt.zones.push(ZoneRef {
                zone: ZoneKind::Battlefield,
                owner: ZoneOwner::Default,
            });
            if ts.eat_phrase(&["or", "in", "the", "command", "zone"]) {
                flt.zones.push(ZoneRef {
                    zone: ZoneKind::CommandZone,
                    owner: ZoneOwner::Default,
                });
            }
            continue;
        }
        if ts.eat_phrase(&["you", "exiled"]) || ts.eat_phrase(&["you've", "exiled"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["at", "the", "beginning", "of", "this", "turn"]) {
            flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
            continue;
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_word("attacking")
                && let Ok(p) = try_parse(ts, parse_player_ref)
            {
                flt.combat = Some(CombatState::Attacking);
                flt.predicates
                    .push(FilterPredicate::AttackedPlayerThisTurn(p));
                continue;
            }
            ts.restore(acp);
        }
        if ts.eat_phrase(&["you've", "cast", "this", "turn"]) {
            flt.controller = Some(ControllerRelation::You);
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CastThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["you've", "drawn", "this", "turn"])
            || ts.eat_phrase(&["you", "drew", "this", "turn"])
        {
            flt.predicates.push(FilterPredicate::DrawnThisTurn);
            continue;
        }
        if ts.eat_phrase(&["you've", "discarded", "this", "turn"])
            || ts.eat_phrase(&["you", "discarded", "this", "turn"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["blocking", "or", "blocked", "by"])
            && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
        {
            flt.combat = Some(CombatState::Blocking);
            flt.predicates
                .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
            continue;
        }
        {
            let bcp = ts.checkpoint();
            if ts.eat_word("blocking")
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.combat = Some(CombatState::Blocking);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
                continue;
            }
            ts.restore(bcp);
            if ts.eat_phrase(&["blocked", "by"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.combat = Some(CombatState::Blocked);
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(ref_subj)));
                continue;
            }
            ts.restore(bcp);
        }
        if ts.eat_phrase(&["chosen", "at", "random"]) || ts.eat_phrase(&["at", "random"]) {
            flt.flags = flt.flags.union(FilterFlags::AT_RANDOM);
            continue;
        }
        if ts.eat_phrase(&["from", "anywhere", "other", "than", "your", "hand"])
            || ts.eat_phrase(&["from", "anywhere", "other", "than", "their", "hand"])
            || ts.eat_phrase(&["from", "anywhere", "other", "than", "exile"])
        {
            flt.predicates.push(FilterPredicate::FromOutsideHand);
            continue;
        }
        if ts.eat_phrase(&["that's", "one", "or", "more", "colors"])
            || ts.eat_phrase(&["that", "are", "one", "or", "more", "colors"])
        {
            flt.color_arity = Some(ColorArity::Colored);
            continue;
        }
        if ts.eat_phrase(&["in", "your", "party"]) {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if ts.eat_phrase(&["of", "that", "type"]) {
            flt.predicates.push(FilterPredicate::OfChosenCreatureType);
            continue;
        }
        if ts.eat_phrase(&["of", "that", "color"]) {
            flt.predicates.push(FilterPredicate::OfThatColor);
            continue;
        }
        if ts.eat_phrase(&["you", "both", "own", "and", "control"]) {
            flt.controller = Some(ControllerRelation::You);
            flt.owner = Some(ControllerRelation::OwnerYou);
            flt.predicates
                .push(FilterPredicate::OwnedAndControlledByYou);
            continue;
        }
        if ts.eat_phrase(&["you", "own", "that", "your", "opponents", "control"])
            || ts.eat_phrase(&["you", "own", "that", "an", "opponent", "controls"])
        {
            flt.owner = Some(ControllerRelation::OwnerYou);
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["controlled", "by", "the", "same", "opponent"])
            || ts.eat_phrase(&["controlled", "by", "the", "same", "player"])
        {
            flt.predicates.push(FilterPredicate::SameController);
            continue;
        }
        if ts.eat_phrase(&["you", "control"]) || ts.eat_phrase(&["you", "cast"]) {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if ts.eat_phrase(&["other", "than"]) && ts.eat_tilde() {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            continue;
        }
        if ts.eat_phrase(&["not", "named"]) && ts.eat_tilde() {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            continue;
        }
        if ts.eat_word("named") && ts.eat_tilde() {
            flt.predicates.push(FilterPredicate::NamedSelf);
            continue;
        }
        if ts.eat_phrase(&["dealt", "damage", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DealtDamageThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&[
            "dealt",
            "damage",
            "this",
            "turn",
            "by",
            "a",
            "source",
            "you",
            "controlled",
        ]) || ts.eat_phrase(&[
            "dealt", "damage", "this", "turn", "by", "a", "source", "you", "control",
        ]) || ts.eat_phrase(&[
            "dealt",
            "damage",
            "by",
            "enchanted",
            "creature",
            "this",
            "turn",
        ]) {
            flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
            continue;
        }
        if ts.eat_phrase(&["dealt", "damage", "by"])
            && ts.eat_tilde()
            && ts.eat_phrase(&["this", "turn"])
        {
            flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
            flt.predicates
                .push(FilterPredicate::DealtDamageBySelfThisTurn);
            continue;
        }
        if ts.eat_phrase(&["your", "opponents", "cast"]) || ts.eat_phrase(&["opponents", "cast"]) {
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "casts"]) {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["you", "control", "but", "don't", "own"])
            || ts.eat_phrase(&["you", "cast", "but", "don't", "own"])
        {
            flt.controller = Some(ControllerRelation::You);
            flt.owner = Some(ControllerRelation::NotYou);
            continue;
        }
        if ts.eat_phrase(&["you", "don't", "own"]) {
            flt.owner = Some(ControllerRelation::NotYou);
            continue;
        }
        if ts.eat_phrase(&["you", "own"]) {
            flt.owner = Some(ControllerRelation::OwnerYou);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "owns"]) || ts.eat_phrase(&["opponents", "own"]) {
            flt.owner = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["attacking", "you"])
            || ts.eat_phrase(&["attacking", "an", "opponent"])
            || ts.eat_phrase(&["attacking", "that", "player"])
        {
            flt.combat = Some(CombatState::Attacking);
            continue;
        }
        if ts.eat_phrase(&["you", "controlled", "as", "you", "cast"]) && ts.eat_tilde() {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if (ts.eat_phrase(&["as", "you", "cast"]) && ts.eat_tilde())
            || ts.eat_phrase(&["as", "you", "activate", "this", "ability"])
        {
            continue;
        }
        if ts.eat_phrase(&["returned", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ReturnedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["sacrificed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::SacrificedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["revealed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::RevealedThisWay,
            ));
            continue;
        }
        {
            let acp = ts.checkpoint();
            if ts.eat_comma()
                && ts.eat_phrase(&["activated", "ability"])
                && ts.eat_comma()
                && ts.eat_phrase(&["or", "triggered", "ability"])
            {
                flt.is_ability = true;
                continue;
            }
            ts.restore(acp);
            if ts.eat_phrase(&["from", "an", "artifact", "or", "enchantment", "source"])
                || ts.eat_phrase(&["from", "an", "artifact", "source"])
                || ts.eat_phrase(&["from", "a", "creature", "source"])
                || ts.eat_phrase(&["from", "a", "noncreature", "source"])
            {
                flt.is_ability = true;
                continue;
            }
            ts.restore(acp);
        }
        {
            let pcp = ts.checkpoint();
            if ts.eat_phrase(&["put", "into"])
                && let Ok(zr) = try_parse(ts, parse_zone_ref)
            {
                if ts.eat_phrase(&["this", "way"]) {
                    flt.zones.push(zr);
                    flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                        PriorActionKind::ReturnedThisWay,
                    ));
                    continue;
                }
                if ts.eat_phrase(&["from", "the", "battlefield", "this", "turn"]) {
                    flt.zones.push(zr);
                    flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                    continue;
                }
            }
            ts.restore(pcp);
        }
        {
            let scp = ts.checkpoint();
            if ts.eat_phrase(&["with", "the", "same", "name", "as"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Name,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            ts.restore(scp);
        }
        if ts.eat_phrase(&["they", "own"]) || ts.eat_phrase(&["that", "player", "owns"]) {
            flt.owner = Some(ControllerRelation::OwnerThatPlayer);
            continue;
        }
        if ts.eat_phrase(&["your", "opponents", "control"])
            || ts.eat_phrase(&["opponents", "control"])
        {
            flt.controller = Some(ControllerRelation::Opponents);
            continue;
        }
        if ts.eat_phrase(&["an", "opponent", "controls"]) {
            flt.controller = Some(ControllerRelation::Opponent);
            continue;
        }
        if ts.eat_phrase(&["target", "player", "controls"]) {
            flt.controller = Some(ControllerRelation::TargetPlayer);
            continue;
        }
        if ts.eat_phrase(&["target", "opponent", "controls"]) {
            flt.controller = Some(ControllerRelation::TargetOpponent);
            continue;
        }
        if ts.eat_phrase(&["they", "control"])
            || ts.eat_phrase(&["that", "player", "controls"])
            || ts.eat_phrase(&[
                "that",
                "player",
                "or",
                "that",
                "planeswalker's",
                "controller",
                "controls",
            ])
        {
            flt.controller = Some(ControllerRelation::ThatPlayer);
            continue;
        }
        if ts.eat_phrase(&["they", "controlled"]) {
            flt.controller = Some(ControllerRelation::ThatPlayer);
            flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
            continue;
        }
        if ts.eat_phrase(&["of", "the", "chosen", "color"]) {
            flt.predicates.push(FilterPredicate::OfChosenColor);
            continue;
        }
        if ts.eat_phrase(&["of", "the", "creature", "type", "of", "your", "choice"])
            || ts.eat_phrase(&["of", "the", "chosen", "creature", "type"])
            || ts.eat_phrase(&["of", "the", "chosen", "type"])
        {
            flt.predicates.push(FilterPredicate::OfChosenCreatureType);
            continue;
        }
        if ts.eat_phrase(&["in", "the", "pile", "of", "their", "choice"])
            || ts.eat_phrase(&["in", "the", "pile", "of", "your", "choice"])
        {
            flt.predicates.push(FilterPredicate::InChosenPile);
            continue;
        }
        if ts.eat_phrase(&["drawn", "this", "turn"]) {
            flt.predicates.push(FilterPredicate::DrawnThisTurn);
            continue;
        }
        if ts.eat_phrase(&["of", "an", "opponent's", "choice"])
            || ts.eat_phrase(&["of", "opponents'", "choice"])
        {
            flt.predicates
                .push(FilterPredicate::OfChosenPlayer(PlayerRef::AnyOpponent));
            continue;
        }
        if ts.eat_phrase(&["of", "their", "choice"]) {
            flt.predicates
                .push(FilterPredicate::OfChosenPlayer(PlayerRef::ThatPlayer));
            continue;
        }
        if ts.eat_phrase(&["of", "your", "choice"]) {
            flt.predicates
                .push(FilterPredicate::OfChosenPlayer(PlayerRef::You));
            continue;
        }
        if ts.eat_phrase(&["exiled", "with"]) && (ts.eat_tilde() || ts.eat_word("it")) {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }
        if ts.eat_phrase(&["from", "among", "the", "cards", "milled", "this", "way"])
            || ts.eat_phrase(&["from", "among", "the", "cards", "exiled", "this", "way"])
            || ts.eat_phrase(&["from", "among", "the", "cards", "revealed", "this", "way"])
            || ts.eat_phrase(&["among", "the", "cards", "milled", "this", "way"])
            || ts.eat_phrase(&["among", "the", "cards", "exiled", "this", "way"])
            || ts.eat_phrase(&["among", "the", "cards", "revealed", "this", "way"])
            || ts.eat_phrase(&["from", "among", "them"])
            || ts.eat_phrase(&["from", "among", "those", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "milled", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "exiled", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "revealed", "cards"])
            || ts.eat_phrase(&["from", "among", "the", "copies"])
            || ts.eat_phrase(&["among", "them"])
            || ts.eat_phrase(&["from", "it"])
            || ts.eat_phrase(&["among", "those", "cards"])
        {
            flt.predicates.push(FilterPredicate::AmongPriorCards);
            continue;
        }
        if (ts.eat_phrase(&["from", "among", "cards", "exiled", "with"])
            || ts.eat_phrase(&["among", "cards", "exiled", "with"]))
            && (ts.eat_tilde() || ts.eat_word("it"))
        {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }

        // Attached-to relative clause (`attached to a Dragon you control`, `that's attached to a permanent`)
        if ts.eat_phrase(&["that's", "attached", "to"]) || ts.eat_phrase(&["attached", "to"]) {
            let host = parse_subject_ref(ts)?;
            flt.predicates
                .push(FilterPredicate::AttachedTo(Box::new(host)));
            continue;
        }

        // `without <kw>` (e.g. `creature without flying`) -- MUST checkpoint so `without paying its mana cost` is never eaten!
        {
            let wcp = ts.checkpoint();
            if ts.eat_word("without")
                && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
            {
                flt.forbidden_keywords
                    .extend(kws.into_iter().map(|k| k.kind));
                continue;
            }
            ts.restore(wcp);
        }

        // Stat bounds & `with different names` & `with <kw>` & `with <counter>` (supports optional `each with ...`)
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with") || w.eq_ignore_ascii_case("each"))
        {
            let cp = ts.checkpoint();
            let _ = ts.eat_word("each");
            if !ts.eat_word("with") {
                ts.restore(cp);
                break;
            }
            if ts.eat_phrase(&["equal", "or", "lesser", "mana", "value"]) {
                flt.predicates
                    .push(FilterPredicate::LesserManaValue { or_equal: true });
                continue;
            }
            if ts.eat_phrase(&["lesser", "mana", "value"]) {
                flt.predicates
                    .push(FilterPredicate::LesserManaValue { or_equal: false });
                continue;
            }
            if ts.eat_phrase(&["the", "chosen", "name"]) || ts.eat_phrase(&["that", "name"]) {
                flt.predicates.push(FilterPredicate::OfChosenName);
                continue;
            }
            if ts.eat_phrase(&["different", "names"]) {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
                continue;
            }
            if ts.eat_phrase(&["different", "powers"]) {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_POWERS);
                continue;
            }
            if matches!(
                ts.peek().map(|t| &t.kind),
                Some(TokenKind::Mana(crate::v2::ir::ManaSymbol::VariableX))
            ) {
                let xcp = ts.checkpoint();
                ts.next_tok();
                if ts.eat_phrase(&["in", "its", "mana", "cost"]) {
                    flt.flags = flt.flags.union(FilterFlags::HAS_X_IN_MANA_COST);
                    continue;
                }
                ts.restore(xcp);
            }
            if ts.eat_word("its")
                && let Some(mc) = ts.eat_mana_cost()
                && ts.eat_word("kicker")
            {
                flt.predicates.push(FilterPredicate::KickedWith(mc));
                continue;
            }
            if ts.eat_phrase(&["the", "same", "name", "as"])
                && let Ok(ref_subj) = try_parse(ts, parse_subject_ref)
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Name,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["the", "same", "name"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Name,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["the", "same", "controller"]) {
                flt.predicates.push(FilterPredicate::SameController);
                continue;
            }
            if ts.eat_phrase(&["the", "same", "mana", "value"]) {
                let ref_subj = if ts.eat_word("as") {
                    try_parse(ts, parse_subject_ref).unwrap_or(SubjectRef::SelfObject)
                } else {
                    SubjectRef::SelfObject
                };
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::ManaValue,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["the", "greatest"])
                && let Some(stat) = parse_single_stat_kind(ts)
            {
                let scope = if ts.eat_word("among") {
                    try_parse(ts, parse_object_filter).unwrap_or_default()
                } else {
                    ObjectFilter::default()
                };
                flt.predicates.push(FilterPredicate::GreatestStat {
                    stat,
                    scope: Box::new(scope),
                });
                continue;
            }
            if ts.eat_phrase(&["the", "least"])
                && let Some(stat) = parse_single_stat_kind(ts)
            {
                let scope = if ts.eat_word("among") {
                    try_parse(ts, parse_object_filter).unwrap_or_default()
                } else {
                    ObjectFilter::default()
                };
                flt.predicates.push(FilterPredicate::LeastStat {
                    stat,
                    scope: Box::new(scope),
                });
                continue;
            }
            if ts.eat_phrase(&["a", "name", "sticker", "on", "it"])
                || ts.eat_phrase(&["a", "name", "sticker", "on", "them"])
            {
                flt.predicates.push(FilterPredicate::HasNameSticker);
                continue;
            }
            if ts.eat_phrase(&["a", "basic", "land", "type"]) {
                flt.predicates.push(FilterPredicate::HasBasicLandType);
                continue;
            }
            if ts.eat_phrase(&["a", "mana", "ability"]) {
                flt.predicates.push(FilterPredicate::HasManaAbility);
                continue;
            }
            {
                let abcp = ts.checkpoint();
                if (ts.eat_word("a") || ts.eat_word("an"))
                    && let Ok(kw) = try_parse(ts, crate::v2::keywords::parse_keyword_kind)
                    && ts.eat_word("ability")
                {
                    flt.predicates.push(FilterPredicate::HasNamedAbility(kw));
                    continue;
                }
                ts.restore(abcp);
            }
            if ts.eat_phrase(&["a", "single", "target"]) {
                flt.flags = flt.flags.union(FilterFlags::SINGLE_TARGET);
                continue;
            }
            // `with counters on them/it` / `with <kind> counters on them/it` (bare plural without article!)
            {
                let ccp = ts.checkpoint();
                if ts.eat_word("counters")
                    && (ts.eat_phrase(&["on", "them"]) || ts.eat_phrase(&["on", "it"]))
                {
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    continue;
                }
                ts.restore(ccp);
                if let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                {
                    let _ = ts.eat_phrase(&["on", "them"]) || ts.eat_phrase(&["on", "it"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                    continue;
                }
                ts.restore(ccp);
            }
            // `with <N> or more <kind> counters on it/them` (`Champion's Drake`)
            {
                let ncp = ts.checkpoint();
                if let Some(_n) = ts.next_number(false)
                    && ts.eat_phrase(&["or", "more"])
                    && let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counters") || ts.eat_word("counter"))
                {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                    continue;
                }
                ts.restore(ncp);
            }
            // `with a [kind] counter on it` / `with one or more [kind] counters on it`
            if ts.eat_phrase(&["one", "or", "more"]) || ts.eat_word("a") || ts.eat_word("an") {
                if ts.eat_word("counter") || ts.eat_word("counters") {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    continue;
                }
                if let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                    flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                    continue;
                }
                ts.restore(cp);
                let _ = ts.eat_word("each");
                ts.eat_word("with");
            }
            if ts.eat_phrase(&["mana", "cost"]) {
                let mut costs = Vec::new();
                if let Some(first_mc) = ts.eat_mana_cost() {
                    costs.push(first_mc);
                    while ts.eat_word("or") {
                        if let Some(next_mc) = ts.eat_mana_cost() {
                            costs.push(next_mc);
                        } else {
                            break;
                        }
                    }
                    flt.predicates.push(FilterPredicate::ManaCostIn(costs));
                    continue;
                }
            }
            // `with power or toughness <cmp> <val>` -> explicit AnyOf([Power, Toughness])
            if ts.eat_phrase(&["power", "or", "toughness"])
                || ts.eat_phrase(&["base", "power", "or", "toughness"])
            {
                let (op, val) = parse_stat_comparison(ts)?;
                let p_branch = ObjectFilter {
                    stat_bounds: vec![StatBound {
                        stat: StatKind::Power,
                        op,
                        value: Box::new(val.clone()),
                    }],
                    ..Default::default()
                };
                let t_branch = ObjectFilter {
                    stat_bounds: vec![StatBound {
                        stat: StatKind::Toughness,
                        op,
                        value: Box::new(val),
                    }],
                    ..Default::default()
                };
                flt.predicates
                    .push(FilterPredicate::AnyOf(vec![p_branch, t_branch]));
                continue;
            }
            if let Some(stat) = parse_single_stat_kind(ts) {
                let (op, val) = parse_stat_comparison(ts)?;
                flt.stat_bounds.push(StatBound {
                    stat,
                    op,
                    value: Box::new(val),
                });
                continue;
            }
            // `with flying` / `with flying or reach`
            if let Ok((kws, is_or)) = try_parse(ts, parse_keyword_list) {
                if is_or && kws.len() >= 2 {
                    flt.keywords_match_any = true;
                }
                flt.required_keywords
                    .extend(kws.into_iter().map(|k| k.kind));
                continue;
            }
            ts.restore(cp);
        }

        // Relative clauses starting with `that's` (`that's a Food or a Golem`, `that's both red and white`)
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("that's"))
        {
            let cp = ts.checkpoint();
            ts.eat_word("that's");
            if ts.eat_word("both") {
                let mut colors = ColorMask::EMPTY;
                if let Some(w1) = ts.peek_word()
                    && let Some(c1) = lookup_color(&w1.to_ascii_lowercase())
                {
                    ts.next_word();
                    colors = colors.union(c1);
                    if ts.eat_word("and")
                        && let Some(w2) = ts.peek_word()
                        && let Some(c2) = lookup_color(&w2.to_ascii_lowercase())
                    {
                        ts.next_word();
                        colors = colors.union(c2);
                        flt.any_colors = flt.any_colors.union(colors);
                        flt.flags = flt.flags.union(FilterFlags::BOTH_COLORS);
                        continue;
                    }
                }
                ts.restore(cp);
            } else {
                let _ = ts.eat_word("a") || ts.eat_word("an");
                let mut subs = Vec::new();
                let mut ctypes = Vec::new();
                let mut saw_or = false;
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                        ts.next_word();
                        subs.push(sub.to_string());
                    } else if let Some(ct) = lookup_card_type(&low) {
                        ts.next_word();
                        ctypes.push(ct);
                    } else if let Some(st) = lookup_supertype(&low) {
                        ts.next_word();
                        flt.required_supertypes = flt.required_supertypes.union(st);
                    } else if let Some(col) = lookup_color(&low) {
                        ts.next_word();
                        flt.any_colors = flt.any_colors.union(col);
                    } else {
                        break;
                    }
                    let _ = ts.eat_comma();
                    if ts.eat_word("or") || ts.eat_word("and/or") {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        saw_or = true;
                    }
                }
                if !subs.is_empty() || !ctypes.is_empty() {
                    if saw_or && subs.len() >= 2 {
                        flt.subtypes_match_any = true;
                    }
                    flt.subtypes.extend(subs);
                    for ct in ctypes {
                        if saw_or {
                            flt.any_types = flt.any_types.union(ct);
                        } else {
                            flt.all_types = flt.all_types.union(ct);
                        }
                    }
                    continue;
                }
                ts.restore(cp);
            }
        }

        // Relative clauses starting with `that`
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("that"))
        {
            let cp = ts.checkpoint();
            ts.eat_word("that");
            if ts.eat_phrase(&[
                "was", "put", "there", "from", "your", "library", "this", "turn",
            ]) || ts.eat_phrase(&[
                "were", "put", "there", "from", "your", "library", "this", "turn",
            ]) {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::MilledThisWay,
                ));
                continue;
            }
            if (ts.eat_phrase(&["has", "a"]) || ts.eat_phrase(&["has", "an"]))
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                continue;
            }
            if ts.eat_phrase(&["weren't", "cast", "this", "way"])
                || ts.eat_phrase(&["wasn't", "cast", "this", "way"])
                || ts.eat_phrase(&["weren't", "cast"])
                || ts.eat_phrase(&["wasn't", "cast"])
                || ts.eat_phrase(&["weren't", "put", "onto", "the", "battlefield"])
                || ts.eat_phrase(&[
                    "weren't",
                    "put",
                    "onto",
                    "the",
                    "battlefield",
                    "this",
                    "way",
                ])
                || ts.eat_phrase(&["weren't", "put", "there", "this", "way"])
                || ts.eat_phrase(&["weren't", "put", "into", "your", "hand"])
                || ts.eat_phrase(&["weren't", "chosen"])
            {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
                continue;
            }
            if ts.eat_phrase(&["isn't", "a", "mana", "ability"])
                || ts.eat_phrase(&["aren't", "mana", "abilities"])
            {
                flt.predicates.push(FilterPredicate::NotManaAbility);
                continue;
            }
            if ts.eat_phrase(&["isn't", "legendary"]) || ts.eat_phrase(&["aren't", "legendary"]) {
                flt.forbidden_supertypes = flt.forbidden_supertypes.union(SupertypeMask::LEGENDARY);
                continue;
            }
            if ts.eat_phrase(&["isn't", "all", "colors"])
                || ts.eat_phrase(&["aren't", "all", "colors"])
            {
                flt.color_arity = Some(ColorArity::Monocolored);
                continue;
            }
            {
                let neg_cp = ts.checkpoint();
                if ts.eat_phrase(&["isn't", "a"])
                    || ts.eat_phrase(&["isn't", "an"])
                    || ts.eat_word("isn't")
                    || ts.eat_word("aren't")
                {
                    let mut f_subs = Vec::new();
                    let mut f_types = CardTypeMask::EMPTY;
                    while let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            f_subs.push(sub.to_string());
                        } else if let Some(ct) = lookup_card_type(&low) {
                            ts.next_word();
                            f_types = f_types.union(ct);
                        } else {
                            break;
                        }
                        let _ = ts.eat_comma();
                        if ts.eat_word("or") || ts.eat_word("and/or") {
                            let _ = ts.eat_word("a") || ts.eat_word("an");
                        }
                    }
                    if !f_subs.is_empty() || !f_types.is_empty() {
                        flt.forbidden_subtypes.extend(f_subs);
                        flt.forbidden_types = flt.forbidden_types.union(f_types);
                        continue;
                    }
                }
                ts.restore(neg_cp);
            }
            if ts.eat_phrase(&["were", "exiled", "this", "way"])
                || ts.eat_phrase(&["was", "exiled", "this", "way"])
            {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
                continue;
            }
            if ts.eat_phrase(&["was", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["were", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["has", "been", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["have", "been", "dealt", "damage", "this", "turn"])
                || ts.eat_phrase(&["dealt", "damage", "to", "you", "this", "turn"])
                || ts.eat_phrase(&["dealt", "combat", "damage", "to", "you", "this", "turn"])
                || ts.eat_phrase(&["dealt", "damage", "to", "a", "player", "this", "turn"])
                || ts.eat_phrase(&["dealt", "damage", "this", "turn"])
            {
                flt.flags = flt.flags.union(FilterFlags::DEALT_DAMAGE_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["entered", "the", "battlefield", "this", "turn"])
                || ts.eat_phrase(&["entered", "this", "turn"])
            {
                flt.flags = flt.flags.union(FilterFlags::ENTERED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["died", "under", "your", "control", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                flt.controller = Some(ControllerRelation::You);
                continue;
            }
            if ts.eat_phrase(&["died", "this", "turn"])
                || ts.eat_phrase(&[
                    "were",
                    "put",
                    "there",
                    "from",
                    "the",
                    "battlefield",
                    "this",
                    "turn",
                ])
                || ts.eat_phrase(&[
                    "was",
                    "put",
                    "there",
                    "from",
                    "the",
                    "battlefield",
                    "this",
                    "turn",
                ])
            {
                flt.flags = flt.flags.union(FilterFlags::DIED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["attacked", "you", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::ATTACKED_THIS_TURN);
                flt.predicates
                    .push(FilterPredicate::AttackedPlayerThisTurn(PlayerRef::You));
                continue;
            }
            if ts.eat_phrase(&["attacked", "this", "turn"]) {
                flt.flags = flt.flags.union(FilterFlags::ATTACKED_THIS_TURN);
                continue;
            }
            if ts.eat_phrase(&["has", "an", "adventure"])
                || ts.eat_phrase(&["have", "an", "adventure"])
            {
                flt.flags = flt.flags.union(FilterFlags::HAS_ADVENTURE);
                continue;
            }
            {
                let ecp = ts.checkpoint();
                if (ts.eat_phrase(&["are", "enchanted", "by"])
                    || ts.eat_phrase(&["is", "enchanted", "by"]))
                    && try_parse(ts, parse_object_filter).is_ok()
                {
                    flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
                    continue;
                }
                ts.restore(ecp);
            }
            if ts.eat_phrase(&["are", "equipped"]) || ts.eat_phrase(&["is", "equipped"]) {
                flt.flags = flt.flags.union(FilterFlags::EQUIPPED);
                continue;
            }
            if ts.eat_phrase(&["are", "enchanted"]) || ts.eat_phrase(&["is", "enchanted"]) {
                flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
                continue;
            }
            if ts.eat_phrase(&["aren't", "on", "the", "battlefield"])
                || ts.eat_phrase(&["isn't", "on", "the", "battlefield"])
            {
                flt.flags = flt.flags.union(FilterFlags::NOT_ON_BATTLEFIELD);
                continue;
            }
            {
                let kwcp = ts.checkpoint();
                if (ts.eat_word("has") || ts.eat_word("have"))
                    && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
                {
                    flt.required_keywords
                        .extend(kws.into_iter().map(|k| k.kind));
                    continue;
                }
                ts.restore(kwcp);
            }
            if ts.eat_word("targets") || ts.eat_word("target") {
                if ts.eat_phrase(&["only", "a", "single"]) || ts.eat_word("only") {
                    flt.flags = flt.flags.union(FilterFlags::SINGLE_TARGET);
                } else {
                    let _ = ts.eat_phrase(&["one", "or", "more"]);
                }
                if let Ok(tgt) = try_parse(ts, parse_subject_ref) {
                    flt.predicates
                        .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
                    continue;
                }
            }
            if ts.eat_phrase(&["is", "a"])
                || ts.eat_phrase(&["is", "an"])
                || ts.eat_word("is")
                || ts.eat_word("are")
            {
                let mut subs = Vec::new();
                let mut ctypes = Vec::new();
                let mut saw_or = false;
                while let Some(w) = ts.peek_word() {
                    let low = w.to_ascii_lowercase();
                    if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                        ts.next_word();
                        subs.push(sub.to_string());
                    } else if let Some(ct) = lookup_card_type(&low) {
                        ts.next_word();
                        ctypes.push(ct);
                    } else if let Some(st) = lookup_supertype(&low) {
                        ts.next_word();
                        flt.required_supertypes = flt.required_supertypes.union(st);
                    } else if let Some(col) = lookup_color(&low) {
                        ts.next_word();
                        flt.any_colors = flt.any_colors.union(col);
                    } else {
                        break;
                    }
                    let _ = ts.eat_comma();
                    if ts.eat_word("or") || ts.eat_word("and/or") {
                        let _ = ts.eat_word("a") || ts.eat_word("an");
                        saw_or = true;
                    }
                }
                if !subs.is_empty() || !ctypes.is_empty() {
                    if saw_or && subs.len() >= 2 {
                        flt.subtypes_match_any = true;
                    }
                    flt.subtypes.extend(subs);
                    for ct in ctypes {
                        if saw_or {
                            flt.any_types = flt.any_types.union(ct);
                        } else {
                            flt.all_types = flt.all_types.union(ct);
                        }
                    }
                    continue;
                }
            }
            if ts.eat_phrase(&["share", "a", "card", "type"])
                && !ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("with"))
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CardType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "permanent", "type"])
                && !ts
                    .peek_word()
                    .is_some_and(|w| w.eq_ignore_ascii_case("with"))
            {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::PermanentType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "creature", "type", "with"])
                || ts.eat_phrase(&["share", "a", "creature", "type", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CreatureType,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "color", "with"])
                || ts.eat_phrase(&["share", "a", "color", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Color,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "card", "type", "with"])
                || ts.eat_phrase(&["share", "a", "card", "type", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CardType,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["shares", "a", "land", "type", "with"])
                || ts.eat_phrase(&["share", "a", "land", "type", "with"])
            {
                let ref_subj = parse_subject_ref(ts)?;
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::LandType,
                    reference: Box::new(ref_subj),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "land", "type"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::LandType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "color"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::Color,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            if ts.eat_phrase(&["share", "a", "creature", "type"]) {
                flt.predicates.push(FilterPredicate::SharesPropertyWith {
                    property: SharedProperty::CreatureType,
                    reference: Box::new(SubjectRef::SelfObject),
                    negated: false,
                });
                continue;
            }
            ts.restore(cp);
        }

        // Trailing prior-action participles (`revealed this way`, `destroyed this way`, `milled this way`, `discarded this way`, `exiled this way`)
        if ts.eat_phrase(&["they", "exiled", "this", "way"])
            || ts.eat_phrase(&["you", "exiled", "this", "way"])
            || ts.eat_phrase(&["that", "player", "exiled", "this", "way"])
            || ts.eat_phrase(&["exiled", "this", "way"])
        {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ExiledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["revealed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::RevealedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["destroyed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DestroyedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["milled", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::MilledThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["discarded", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::DiscardedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["sacrificed", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::SacrificedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["tapped", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::TappedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["returned", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::ReturnedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["created", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CreatedThisWay,
            ));
            continue;
        }
        if ts.eat_phrase(&["cast", "this", "way"]) {
            flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                PriorActionKind::CastThisWay,
            ));
            continue;
        }

        // Zone source clauses (`from your graveyard`, `from exile and from your graveyard`,
        // `from your hand or from the command zone`, `in your graveyard`)
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("from") || w.eq_ignore_ascii_case("in"))
        {
            let cp = ts.checkpoint();
            let prep = ts.next_word().expect("peeked").to_ascii_lowercase();
            // Do not steal `from <src> onto <dst>` when inside `MoveCounters`
            if let Ok(zref) = try_parse(ts, parse_zone_ref) {
                flt.zones.push(zref);
                // Multi-zone conjunction (`and from <z2>`, `or from <z2>`, `or graveyard`, `and exile`)
                loop {
                    let m_cp = ts.checkpoint();
                    if ts.eat_word("and") || ts.eat_word("or") {
                        let _ = ts.eat_word(&prep);
                        if let Ok(z2) = try_parse(ts, parse_zone_ref) {
                            flt.zones.push(z2);
                            continue;
                        }
                        let bare_z = if ts.eat_word("graveyard") || ts.eat_word("graveyards") {
                            Some(ZoneKind::Graveyard)
                        } else if ts.eat_word("hand") || ts.eat_word("hands") {
                            Some(ZoneKind::Hand)
                        } else if ts.eat_word("library") || ts.eat_word("libraries") {
                            Some(ZoneKind::Library)
                        } else if ts.eat_word("exile") {
                            Some(ZoneKind::Exile)
                        } else {
                            None
                        };
                        if let Some(zone) = bare_z {
                            flt.zones.push(ZoneRef {
                                zone,
                                owner: zref.owner,
                            });
                            continue;
                        }
                    }
                    ts.restore(m_cp);
                    break;
                }
                continue;
            }
            ts.restore(cp);
        }

        break;
    }
    Ok(())
}

fn parse_single_stat_kind(ts: &mut TokenStream<'_>) -> Option<StatKind> {
    if ts.eat_phrase(&["mana", "value"]) || ts.eat_phrase(&["total", "mana", "value"]) {
        Some(StatKind::ManaValue)
    } else if ts.eat_phrase(&["base", "power"]) || ts.eat_word("power") {
        Some(StatKind::Power)
    } else if ts.eat_phrase(&["base", "toughness"]) || ts.eat_word("toughness") {
        Some(StatKind::Toughness)
    } else if ts.eat_word("loyalty") {
        Some(StatKind::Loyalty)
    } else {
        None
    }
}

fn parse_stat_comparison(ts: &mut TokenStream<'_>) -> PResult<(CompareOp, ValueExpr)> {
    if ts.eat_phrase(&["equal", "to"]) {
        return Ok((CompareOp::Eq, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["less", "than", "or", "equal", "to"]) {
        return Ok((CompareOp::Le, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["greater", "than", "or", "equal", "to"]) {
        return Ok((CompareOp::Ge, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["less", "than"]) {
        return Ok((CompareOp::Lt, parse_value_expr(ts)?));
    }
    if ts.eat_phrase(&["greater", "than"]) {
        return Ok((CompareOp::Gt, parse_value_expr(ts)?));
    }
    let val = parse_value_expr(ts)?;
    if ts.eat_phrase(&["or", "less"]) {
        Ok((CompareOp::Le, val))
    } else if ts.eat_phrase(&["or", "greater"]) {
        Ok((CompareOp::Ge, val))
    } else {
        Ok((CompareOp::Eq, val))
    }
}
