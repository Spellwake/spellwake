//! Strictly-typed compositional parsers for players, keywords, counters, zones,
//! [`ObjectFilter`] (explicit AND `all_types` vs OR `any_types`), and [`SubjectRef`].

use crate::ast::{
    CardTypeMask, ColorArity, ColorMask, CombatState, CompareOp, ControllerRelation, StatKind,
    SupertypeMask,
};
use crate::v2::ir::{
    AnaphoraRef, CountSpec, CounterKind, EntryModifiers, FilterFlags, FilterPredicate,
    LibraryOrder, LibraryPosition, ObjectFilter, PlayerRef, PriorActionKind, SharedProperty,
    StatBound, SubjectRef, TargetCount, ValueExpr, ZoneDest, ZoneKind, ZoneOwner, ZoneRef,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, try_parse};
use crate::v2::value::parse_value_expr;
use crate::vocab::{lookup_card_type, lookup_color, lookup_supertype};

/// Parse a typed player reference.
pub fn parse_player_ref(ts: &mut TokenStream<'_>) -> PResult<PlayerRef> {
    if ts.eat_phrase(&["the", "player", "with", "the", "lowest", "life", "total"]) {
        return Ok(PlayerRef::LowestLifePlayer);
    }
    if ts.eat_phrase(&["the", "player", "with", "the", "highest", "life", "total"]) {
        return Ok(PlayerRef::HighestLifePlayer);
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
    if ts.eat_phrase(&["each", "player"]) || ts.eat_phrase(&["all", "players"]) {
        return Ok(PlayerRef::EachPlayer);
    }
    if ts.eat_phrase(&["target", "opponent"]) {
        return Ok(PlayerRef::TargetOpponent);
    }
    if ts.eat_phrase(&["target", "player"]) {
        return Ok(PlayerRef::TargetPlayer);
    }
    if ts.eat_phrase(&["an", "opponent"]) || ts.eat_phrase(&["any", "opponent"]) {
        return Ok(PlayerRef::AnyOpponent);
    }
    if ts.eat_phrase(&["a", "player"]) || ts.eat_phrase(&["any", "player"]) {
        return Ok(PlayerRef::AnyPlayer);
    }
    if ts.eat_phrase(&["that", "player"])
        || ts.eat_phrase(&["the", "player"])
        || ts.eat_word("they")
    {
        return Ok(PlayerRef::ThatPlayer);
    }
    if ts.eat_phrase(&["defending", "player"]) {
        return Ok(PlayerRef::DefendingPlayer);
    }
    if ts.eat_phrase(&["active", "player"]) {
        return Ok(PlayerRef::ActivePlayer);
    }
    if ts.eat_phrase(&["enchanted", "player"]) || ts.eat_phrase(&["enchanted", "opponent"]) {
        return Ok(PlayerRef::EnchantedPlayer);
    }
    if ts.eat_phrase(&["that", "creature's", "controller"])
        || ts.eat_phrase(&["that", "permanent's", "controller"])
        || ts.eat_phrase(&["that", "card's", "owner"])
    {
        return Ok(PlayerRef::ItsController);
    }
    if ts.eat_phrase(&["that", "spell's", "controller"]) {
        return Ok(PlayerRef::ThatSpellController);
    }
    if ts.eat_phrase(&["its", "controller"]) {
        return Ok(PlayerRef::ItsController);
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
    let owner = if ts.eat_word("your") {
        ZoneOwner::You
    } else if ts.eat_phrase(&["target", "opponent's"]) {
        ZoneOwner::TargetOpponent
    } else if ts.eat_phrase(&["target", "player's"]) {
        ZoneOwner::TargetPlayer
    } else if ts.eat_phrase(&["that", "player's"]) || ts.eat_word("their") {
        ZoneOwner::ThatPlayer
    } else if ts.eat_phrase(&["that", "creature's", "owner's"])
        || ts.eat_phrase(&["its", "owner's"])
    {
        ZoneOwner::ItsOwner
    } else if ts.eat_phrase(&["their", "owners'"]) {
        ZoneOwner::TheirOwners
    } else if ts.eat_phrase(&["an", "opponent's"]) {
        ZoneOwner::Opponent
    } else if ts.eat_phrase(&["your", "opponents'"]) || ts.eat_word("opponents'") {
        ZoneOwner::Opponents
    } else if ts.eat_phrase(&["each", "player's"]) {
        ZoneOwner::EachPlayer
    } else if ts.eat_word("all") {
        ZoneOwner::All
    } else if ts.eat_word("any") || ts.eat_word("a") || ts.eat_word("that") {
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

    // Library position destinations: `the top of <owner> library` / `the bottom of <owner> library`
    if ts.eat_phrase(&["the", "top", "of"]) || ts.eat_phrase(&["top", "of"]) {
        let zref = parse_zone_ref(ts)?;
        if zref.zone != ZoneKind::Library {
            ts.restore(cp);
            return Err(ts.err("expected library after `top of`"));
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
        if ts.eat_word("transformed") {
            modifiers.transformed = true;
            continue;
        }
        if ts.eat_phrase(&["under", "your", "control"]) {
            under_control = Some(ZoneOwner::You);
            continue;
        }
        if ts.eat_phrase(&["under", "their", "owners'", "control"]) {
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
            library_position: None,
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
            // Allow Oxford comma inside disjunctive card-type lists (`artifact, creature, or enchantment`)
            if !card_types_seen.is_empty() {
                if ts.peek_word().is_some_and(|w| w.eq_ignore_ascii_case("or"))
                    && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if lookup_card_type(&nw.to_ascii_lowercase()).is_some())
                    })
                {
                    pending_or_type = true;
                    continue;
                }
                if ts
                    .peek_word()
                    .is_some_and(|w| lookup_card_type(&w.to_ascii_lowercase()).is_some())
                    && matches!(ts.peek_n(1).map(|t| &t.kind), Some(TokenKind::Comma))
                    && matches!(ts.peek_n(2).map(|t| &t.kind), Some(TokenKind::Word(nw)) if nw.eq_ignore_ascii_case("or"))
                {
                    pending_or_type = true;
                    continue;
                }
            }
            ts.restore(cp);
            break;
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
                        )
                }
                _ => false,
            });
            if next_ok {
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
            "other" | "another" => {
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
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
                ts.next_word();
                flt.tapped = Some(true);
            }
            "untapped" => {
                ts.next_word();
                flt.tapped = Some(false);
            }
            "attacking" => {
                ts.next_word();
                flt.combat = Some(if flt.combat == Some(CombatState::Blocking) {
                    CombatState::AttackingOrBlocking
                } else {
                    CombatState::Attacking
                });
            }
            "blocking" => {
                ts.next_word();
                flt.combat = Some(if flt.combat == Some(CombatState::Attacking) {
                    CombatState::AttackingOrBlocking
                } else {
                    CombatState::Blocking
                });
            }
            "blocked" => {
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
            continue;
        }
        if ts.eat_phrase(&["you", "control"]) {
            flt.controller = Some(ControllerRelation::You);
            continue;
        }
        if ts.eat_phrase(&["you", "own"]) {
            flt.owner = Some(ControllerRelation::OwnerYou);
            continue;
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
        if ts.eat_phrase(&["they", "control"]) || ts.eat_phrase(&["that", "player", "controls"]) {
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
        if ts.eat_phrase(&["of", "the", "chosen", "creature", "type"])
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
        if ts.eat_phrase(&["exiled", "with"]) && ts.eat_tilde() {
            flt.flags = flt.flags.union(FilterFlags::EXILED_WITH_SELF);
            continue;
        }
        if ts.eat_phrase(&["from", "among", "them"]) || ts.eat_phrase(&["among", "them"]) {
            flt.predicates.push(FilterPredicate::AmongPriorCards);
            continue;
        }

        // Attached-to relative clause (`attached to a Dragon you control`)
        if ts.eat_phrase(&["attached", "to"]) {
            let host = parse_subject_ref(ts)?;
            flt.predicates
                .push(FilterPredicate::AttachedTo(Box::new(host)));
            continue;
        }

        // `without <kw>` (e.g. `creature without flying`)
        if ts.eat_word("without")
            && let Ok((kws, _)) = try_parse(ts, parse_keyword_list)
        {
            flt.forbidden_keywords
                .extend(kws.into_iter().map(|k| k.kind));
            continue;
        }

        // Stat bounds & `with different names` & `with <kw>` & `with <counter>`
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let cp = ts.checkpoint();
            ts.eat_word("with");
            if ts.eat_phrase(&["different", "names"]) {
                flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
                continue;
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

        // Relative clauses starting with `that`
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("that"))
        {
            let cp = ts.checkpoint();
            ts.eat_word("that");
            if (ts.eat_phrase(&["has", "a"]) || ts.eat_phrase(&["has", "an"]))
                && let Ok(ck) = try_parse(ts, parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
            {
                let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                flt.flags = flt.flags.union(FilterFlags::WITH_ANY_COUNTER);
                flt.predicates.push(FilterPredicate::HasCounterKind(ck));
                continue;
            }
            if ts.eat_phrase(&["were", "exiled", "this", "way"])
                || ts.eat_phrase(&["was", "exiled", "this", "way"])
            {
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    PriorActionKind::ExiledThisWay,
                ));
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
            if (ts.eat_word("targets") || ts.eat_word("target"))
                && let Ok(tgt) = try_parse(ts, parse_subject_ref)
            {
                flt.predicates
                    .push(FilterPredicate::TargetsSubject(Box::new(tgt)));
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
            ts.restore(cp);
        }

        // Trailing `[they|you] exiled this way` without `that were`
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
                // Multi-zone conjunction (`and from <z2>`, `or from <z2>`)
                loop {
                    let m_cp = ts.checkpoint();
                    if (ts.eat_word("and") || ts.eat_word("or"))
                        && ts.eat_word(&prep)
                        && let Ok(z2) = try_parse(ts, parse_zone_ref)
                    {
                        flt.zones.push(z2);
                        continue;
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

/// Strictly-typed [`SubjectRef`] parser (handles `~`, `any target`, `target ...`, `up to N target ...`,
/// `another target ...`, `all ...`, `each ...`, anaphoric references, and players).
pub fn parse_subject_ref(ts: &mut TokenStream<'_>) -> PResult<SubjectRef> {
    if ts.eat_tilde() {
        return Ok(SubjectRef::SelfObject);
    }
    if ts.eat_phrase(&["any", "target"]) {
        return Ok(SubjectRef::AnyTarget);
    }
    // `equipped creature` / `enchanted creature` / `enchanted permanent`
    if ts.eat_phrase(&["equipped", "creature"]) {
        return Ok(SubjectRef::Anaphora(AnaphoraRef::EquippedCreature));
    }
    if ts.eat_phrase(&["enchanted", "creature"]) {
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
            if ts.eat_word("x") {
                Some(TargetCount::UpToX)
            } else {
                ts.next_number(false)
                    .and_then(|n| u16::try_from(n).ok())
                    .map(TargetCount::UpTo)
            }
        } else if ts.eat_phrase(&["any", "number", "of"]) {
            Some(TargetCount::AnyNumber)
        } else if ts.eat_word("another") {
            another = true;
            Some(TargetCount::Exact(1))
        } else if let Some(n) = ts.next_number(false) {
            u16::try_from(n).ok().map(TargetCount::Exact)
        } else {
            None
        };

        if ts.eat_word("target") {
            let count = count_opt.unwrap_or(TargetCount::Exact(1));
            if ts.eat_word("opponent") || ts.eat_word("opponents") {
                return Ok(SubjectRef::TargetPlayer {
                    opponent_only: true,
                    count,
                });
            }
            if ts.eat_word("player") || ts.eat_word("players") {
                if ts.eat_phrase(&["or", "planeswalker"]) {
                    return Ok(SubjectRef::Compound(vec![
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
                return Ok(SubjectRef::TargetPlayer {
                    opponent_only: false,
                    count,
                });
            }
            let mut flt = parse_object_filter(ts)?;
            if another {
                flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            }
            // Support trailing `or player` (`target creature or player`, `target creature, planeswalker, or player`)
            if ts.eat_phrase(&["or", "player"]) {
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
            return Ok(SubjectRef::TargetObject {
                filter: Box::new(flt),
                count,
                another,
            });
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
            return Ok(SubjectRef::Player(p));
        }
        ts.restore(cp);
    }

    // `each <filter>`
    if ts.eat_word("each") {
        let flt = parse_object_filter(ts)?;
        return Ok(SubjectRef::Each(Box::new(flt)));
    }

    // Strictly-typed anaphora references
    if let Some(an) = parse_anaphora(ts) {
        return Ok(SubjectRef::Anaphora(an));
    }

    // Bare object filter group (`creatures you control`, `a Dragon creature card ...`)
    let flt = parse_object_filter(ts)?;
    Ok(SubjectRef::Group(Box::new(flt)))
}

/// Parse a strictly-typed anaphora token/phrase.
pub fn parse_anaphora(ts: &mut TokenStream<'_>) -> Option<AnaphoraRef> {
    if ts.eat_phrase(&["the", "rest"]) {
        return Some(AnaphoraRef::TheRest);
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
    if ts.eat_phrase(&["that", "creature"]) {
        return Some(AnaphoraRef::ThatCreature);
    }
    if ts.eat_phrase(&["that", "permanent"]) {
        return Some(AnaphoraRef::ThatPermanent);
    }
    if ts.eat_phrase(&["that", "card"]) || ts.eat_phrase(&["the", "card"]) {
        return Some(AnaphoraRef::ThatCard);
    }
    if ts.eat_phrase(&["that", "spell"]) {
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
    if ts.eat_word("it") {
        return Some(AnaphoraRef::It);
    }
    if ts.eat_word("them") {
        return Some(AnaphoraRef::Them);
    }
    None
}
