//! Strictly-typed compositional parsers for players, keywords, counters, zones,
//! [`ObjectFilter`] (explicit AND `all_types` vs OR `any_types`), and [`SubjectRef`].

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, StatKind, SupertypeMask,
};
use crate::v2::ir::{
    CounterKind, EntryModifiers, FilterFlags, FilterPredicate, LibraryOrder, LibraryPosition,
    ObjectFilter, PlayerRef, StatBound, ValueExpr, ZoneDest, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::vocab::{lookup_card_type, lookup_color, lookup_supertype};

mod post_modifiers;
/// Parse a typed player reference.
mod subject;
use post_modifiers::parse_filter_post_modifiers;

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
        || ts.eat_phrase(&["those", "players"])
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
    } else if ts.eat_phrase(&["an", "opponent's"])
        || ts.eat_phrase(&["each", "opponent's"])
        || ts.eat_phrase(&["defending", "player's"])
    {
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
        if library_position.is_some()
            && (ts.eat_phrase(&["or", "on", "the", "bottom"])
                || ts.eat_phrase(&["or", "on", "the", "bottom", "of", "their", "library"])
                || ts.eat_phrase(&[
                    "or", "on", "the", "bottom", "of", "its", "owner's", "library",
                ]))
        {
            library_position = Some(LibraryPosition::TopOrBottom);
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
    if ts.eat_phrase(&["your", "commanders"]) || ts.eat_phrase(&["your", "commander"]) {
        flt.any_permanent = true;
        flt.controller = Some(crate::ast::ControllerRelation::You);
        flt.flags = flt.flags.union(FilterFlags::IS_COMMANDER);
        parse_filter_post_modifiers(ts, &mut flt)?;
        return Ok(flt);
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
                let is_verb_tok = |tok: Option<&crate::v2::lexer::Token<'_>>| {
                    tok.is_some_and(|t| {
                        matches!(
                            t.kind,
                            TokenKind::Word(v)
                                if matches!(
                                    v.to_ascii_lowercase().as_str(),
                                    "entered"
                                        | "enters"
                                        | "enter"
                                        | "died"
                                        | "dies"
                                        | "die"
                                        | "left"
                                        | "leaves"
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
                    })
                };
                let is_cond_clause = matches!(
                    ts.peek_n(1).map(|t| &t.kind),
                    Some(TokenKind::Word(w)) if w.eq_ignore_ascii_case("a") || w.eq_ignore_ascii_case("an")
                ) && (is_verb_tok(ts.peek_n(3))
                    || (ts.peek_n(3).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("you"))
                    }) && ts.peek_n(4).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("control"))
                    }) && ts.peek_n(5).is_some_and(|t| {
                        matches!(
                            t.kind,
                            TokenKind::Word(w)
                                if matches!(
                                    w.to_ascii_lowercase().as_str(),
                                    "enters" | "enter" | "dies" | "die" | "leaves"
                                )
                        )
                    })));
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
            "exiled" => {
                // Leave post-nominal `cards exiled this way` / `cards exiled with ~` for `parse_filter_post_modifiers`.
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
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    crate::v2::ir::PriorActionKind::ExiledThisWay,
                ));
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
