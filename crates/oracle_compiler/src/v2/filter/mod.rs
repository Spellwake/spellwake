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
mod relatives;
/// Parse a typed player reference.
mod subject;
use post_modifiers::parse_filter_post_modifiers;

pub use subject::parse_subject_ref;

pub(super) fn eat_phrase_table<T: Copy>(
    ts: &mut TokenStream<'_>,
    table: &[(&[&str], T)],
) -> Option<T> {
    table
        .iter()
        .find_map(|&(phrase, val)| ts.eat_phrase(phrase).then_some(val))
}

pub(super) fn eat_phrase_groups<T: Copy>(
    ts: &mut TokenStream<'_>,
    groups: &[(&[&[&str]], T)],
) -> Option<T> {
    groups
        .iter()
        .find_map(|&(phrases, val)| ts.eat_any_phrase(phrases).then_some(val))
}

pub(super) fn push_exclude_filter(flt: &mut ObjectFilter, ex: ObjectFilter) {
    flt.predicates.push(FilterPredicate::Exclude(Box::new(
        crate::v2::ir::SubjectRef::Group(Box::new(ex)),
    )));
}

pub(super) fn push_stat_bound(
    flt: &mut ObjectFilter,
    stat: StatKind,
    op: CompareOp,
    value: ValueExpr,
) {
    flt.stat_bounds.push(StatBound {
        stat,
        op,
        value: Box::new(value),
    });
}

pub(super) fn push_shares_property(
    flt: &mut ObjectFilter,
    property: crate::v2::ir::SharedProperty,
    reference: crate::v2::ir::SubjectRef,
    negated: bool,
) {
    flt.predicates.push(FilterPredicate::SharesPropertyWith {
        property,
        reference: Box::new(reference),
        negated,
    });
}

const PLAYER_REF_HEAD: &[(&[&[&str]], PlayerRef)] = &[
    (
        &[&["the", "player", "whose", "turn", "it", "is"]],
        PlayerRef::ActivePlayer,
    ),
    (
        &[
            &["the", "player", "with", "the", "lowest", "life", "total"],
            &["a", "player", "with", "the", "lowest", "life", "total"],
        ],
        PlayerRef::LowestLifePlayer,
    ),
    (
        &[
            &["the", "player", "with", "the", "highest", "life", "total"],
            &[
                "the", "player", "with", "the", "most", "life", "or", "tied", "for", "most", "life",
            ],
            &[
                "a", "player", "with", "the", "most", "life", "or", "tied", "for", "most", "life",
            ],
            &["the", "player", "with", "the", "most", "life"],
            &["a", "player", "with", "the", "most", "life"],
        ],
        PlayerRef::HighestLifePlayer,
    ),
    (
        &[
            &[
                "the", "player", "with", "the", "most", "cards", "in", "hand",
            ],
            &[
                "the", "opponent", "with", "the", "most", "cards", "in", "hand",
            ],
        ],
        PlayerRef::MostObjectsPlayer,
    ),
];

const PLAYER_REF_MID: &[(&[&[&str]], PlayerRef)] = &[
    (
        &[&["each", "opponent"], &["your", "opponents"]],
        PlayerRef::EachOpponent,
    ),
    (
        &[&["each", "player"], &["all", "players"], &["players"]],
        PlayerRef::EachPlayer,
    ),
    (
        &[
            &[
                "that",
                "player",
                "or",
                "that",
                "planeswalker's",
                "controller",
            ],
            &["that", "player", "or", "that", "permanent's", "controller"],
            &[
                "that",
                "opponent",
                "or",
                "that",
                "planeswalker's",
                "controller",
            ],
            &[
                "that",
                "player",
                "or",
                "by",
                "that",
                "planeswalker's",
                "controller",
            ],
            &[
                "target",
                "player",
                "or",
                "that",
                "planeswalker's",
                "controller",
            ],
            &[
                "target",
                "player",
                "or",
                "that",
                "permanent's",
                "controller",
            ],
        ],
        PlayerRef::ThatPlayer,
    ),
    (
        &[
            &["that", "spell", "or", "ability's", "controller"],
            &["that", "spell's", "or", "ability's", "controller"],
        ],
        PlayerRef::ThatSpellController,
    ),
    (
        &[
            &["that", "creature's", "or", "spell's", "controller"],
            &["that", "spell", "or", "permanent's", "controller"],
            &["that", "permanent", "or", "spell's", "controller"],
        ],
        PlayerRef::ItsController,
    ),
];

const PLAYER_REF_TAIL: &[(&[&[&str]], PlayerRef)] = &[
    (&[&["target", "opponent"]], PlayerRef::TargetOpponent),
    (&[&["target", "player"]], PlayerRef::TargetPlayer),
    (
        &[
            &["an", "opponent"],
            &["any", "opponent"],
            &["one", "of", "your", "opponents"],
        ],
        PlayerRef::AnyOpponent,
    ),
    (
        &[&["a", "player"], &["any", "player"]],
        PlayerRef::AnyPlayer,
    ),
    (
        &[
            &["that", "attacking", "player"],
            &["that", "player"],
            &["that", "opponent"],
            &["the", "player"],
            &["those", "players"],
            &["they"],
        ],
        PlayerRef::ThatPlayer,
    ),
    (&[&["defending", "player"]], PlayerRef::DefendingPlayer),
    (
        &[
            &["active", "player"],
            &["the", "attacking", "player"],
            &["attacking", "player"],
        ],
        PlayerRef::ActivePlayer,
    ),
    (
        &[&["enchanted", "player"], &["enchanted", "opponent"]],
        PlayerRef::EnchantedPlayer,
    ),
    (
        &[
            &["that", "creature's", "controller"],
            &["that", "permanent's", "controller"],
            &["the", "creature's", "controller"],
            &["the", "permanent's", "controller"],
            &["that", "land's", "controller"],
            &["that", "artifact's", "controller"],
            &["that", "source's", "controller"],
            &["the", "source's", "controller"],
            &["that", "source's", "owner"],
            &["the", "source's", "owner"],
            &["the", "exiled", "card's", "owner"],
            &["the", "exiled", "card's", "controller"],
            &["that", "card's", "owner"],
            &["that", "creature's", "owner"],
            &["target", "creature's", "controller"],
            &["target", "artifact", "creature's", "controller"],
            &["target", "permanent's", "controller"],
            &["enchanted", "creature's", "controller"],
            &["enchanted", "permanent's", "controller"],
            &["target", "creature's", "owner"],
            &["target", "permanent's", "owner"],
            &["target", "nonland", "permanent's", "owner"],
            &["target", "spell's", "controller"],
        ],
        PlayerRef::ItsController,
    ),
];

pub fn parse_player_ref(ts: &mut TokenStream<'_>) -> PResult<PlayerRef> {
    if let Some(p) = eat_phrase_groups(ts, PLAYER_REF_HEAD) {
        return Ok(p);
    }
    if ts.eat_phrase(&[
        "each", "player", "who", "searched", "their", "library", "this", "way",
    ]) {
        return Ok(PlayerRef::SearchedLibraryPlayer);
    }
    if ts.eat_phrase(&["each", "other", "opponent"]) {
        return Ok(PlayerRef::EachOtherOpponent);
    }
    if ts.eat_any_phrase(&[
        &["each", "other", "player"],
        &["another", "player"],
        &["other", "players"],
        &["each", "player", "other", "than", "you"],
        &["any", "player", "other", "than", "you"],
        &["a", "player", "other", "than", "you"],
    ]) {
        return Ok(PlayerRef::EachOtherPlayer);
    }
    if let Some(p) = eat_phrase_groups(ts, PLAYER_REF_MID) {
        return Ok(p);
    }
    if let Some(poss_p) = try_parse_possessive_object_player_ref(ts) {
        return Ok(poss_p);
    }
    if let Some(p) = eat_phrase_groups(ts, PLAYER_REF_TAIL) {
        return Ok(p);
    }
    {
        let t_cp = ts.checkpoint();
        if ts.eat_word("target") && try_parse(ts, parse_object_filter).is_ok() && ts.eat_word("'s")
        {
            if ts.eat_word("owner") {
                return Ok(PlayerRef::ItsOwner);
            }
            if ts.eat_word("controller") {
                return Ok(PlayerRef::ItsController);
            }
        }
        ts.restore(t_cp);
    }
    {
        let ocp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_word("'s") {
            if ts.eat_word("owner") {
                return Ok(PlayerRef::ItsOwner);
            }
            if ts.eat_word("controller") {
                return Ok(PlayerRef::ItsController);
            }
        }
        ts.restore(ocp);
    }
    if ts.eat_phrase(&["that", "spell's", "controller"]) {
        return Ok(PlayerRef::ThatSpellController);
    }
    if ts.eat_phrase(&["the", "controller", "of"]) {
        let _ = try_parse(ts, parse_subject_ref);
        return Ok(PlayerRef::ItsController);
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

/// Match possessive object-owner/controller forms where the lexer fuses `'s` onto the noun
/// (e.g. `target artifact or creature's owner`, `the Wall's controller`, `that Aura's controller`).
fn try_parse_possessive_object_player_ref(ts: &mut TokenStream<'_>) -> Option<PlayerRef> {
    let cp = ts.checkpoint();
    if !(ts.eat_word("target")
        || ts.eat_word("that")
        || ts.eat_word("the")
        || ts.eat_word("enchanted")
        || ts.eat_word("equipped"))
    {
        return None;
    }
    // Scan up to 6 object-noun/modifier words until we hit a possessive noun (`...'s`)
    for _ in 0..6 {
        let Some(w) = ts.peek_word() else {
            break;
        };
        let low = w.to_ascii_lowercase();
        if let Some(stem) = low.strip_suffix("'s") {
            let is_obj_noun = lookup_card_type(stem).is_some()
                || ts.vocab.lookup_subtype(stem).is_some()
                || matches!(
                    stem,
                    "permanent" | "spell" | "ability" | "card" | "source" | "token" | "object"
                );
            if is_obj_noun {
                ts.next_word();
                if ts.eat_word("owner") {
                    return Some(PlayerRef::ItsOwner);
                }
                if ts.eat_word("controller") {
                    return Some(if stem == "spell" {
                        PlayerRef::ThatSpellController
                    } else {
                        PlayerRef::ItsController
                    });
                }
                break;
            }
            // Handle `that creature's or spell's controller`
            ts.restore(cp);
            return None;
        }
        if is_filter_modifier_word(w, ts.vocab)
            || matches!(
                low.as_str(),
                "or" | "and/or"
                    | "permanent"
                    | "card"
                    | "spell"
                    | "ability"
                    | "source"
                    | "token"
                    | "exiled"
                    | "sacrificed"
                    | "destroyed"
                    | "discarded"
                    | "revealed"
                    | "chosen"
            )
        {
            ts.next_word();
            continue;
        }
        break;
    }
    ts.restore(cp);
    None
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
    const FIXED_ZONES: &[(&[&[&str]], ZoneKind)] = &[
        (&[&["exile"]], ZoneKind::Exile),
        (
            &[&["the", "battlefield"], &["battlefield"]],
            ZoneKind::Battlefield,
        ),
        (
            &[&["the", "command", "zone"], &["command", "zone"]],
            ZoneKind::CommandZone,
        ),
        (&[&["the", "stack"], &["stack"]], ZoneKind::Stack),
        (&[&["outside", "the", "game"]], ZoneKind::OutsideTheGame),
        (
            &[
                &["among", "the", "exiled", "cards"],
                &["among", "those", "exiled", "cards"],
            ],
            ZoneKind::Exile,
        ),
    ];
    if let Some(zone) = eat_phrase_groups(ts, FIXED_ZONES) {
        return Ok(ZoneRef {
            zone,
            owner: ZoneOwner::Default,
        });
    }
    // `the hand/graveyard/library of <player>`
    {
        let zcp = ts.checkpoint();
        const OF_ZONES: &[(&[&str], ZoneKind)] = &[
            (&["the", "hand", "of"], ZoneKind::Hand),
            (&["the", "graveyard", "of"], ZoneKind::Graveyard),
            (&["the", "library", "of"], ZoneKind::Library),
        ];
        if let Some(zone) = eat_phrase_table(ts, OF_ZONES)
            && let Ok(player) = try_parse(ts, parse_player_ref)
        {
            let owner = match player {
                PlayerRef::You => Some(ZoneOwner::You),
                PlayerRef::ThatPlayer | PlayerRef::ChosenPlayer => Some(ZoneOwner::ThatPlayer),
                PlayerRef::TargetPlayer => Some(ZoneOwner::TargetPlayer),
                PlayerRef::TargetOpponent => Some(ZoneOwner::TargetOpponent),
                PlayerRef::AnyOpponent
                | PlayerRef::DefendingPlayer
                | PlayerRef::EnchantedPlayer => Some(ZoneOwner::Opponent),
                PlayerRef::EachOpponent | PlayerRef::EachOtherOpponent => {
                    Some(ZoneOwner::Opponents)
                }
                PlayerRef::EachPlayer => Some(ZoneOwner::EachPlayer),
                PlayerRef::AnyPlayer => Some(ZoneOwner::Any),
                PlayerRef::ItsOwner | PlayerRef::ItsController => Some(ZoneOwner::ItsOwner),
                _ => None,
            };
            if let Some(owner) = owner {
                return Ok(ZoneRef { zone, owner });
            }
        }
        ts.restore(zcp);
    }

    let cp = ts.checkpoint();
    // Longest-prefix match FIRST so `your opponents'` isn't shadowed by `your`
    // and `their owners'` / `their owner's` isn't shadowed by `their`!
    const ZONE_OWNERS: &[(&[&[&str]], ZoneOwner)] = &[
        (
            &[
                &["your", "opponents'"],
                &["opponents'"],
                &["other", "players'"],
                &["each", "other", "player's"],
            ],
            ZoneOwner::Opponents,
        ),
        (
            &[
                &["their", "owners'"],
                &["their", "owner's"],
                &["their", "controllers'"],
                &["their", "controller's"],
            ],
            ZoneOwner::TheirOwners,
        ),
        (
            &[
                &["that", "creature's", "owner's"],
                &["that", "permanent's", "owner's"],
                &["that", "card's", "owner's"],
                &["its", "owner's"],
                &["her", "owner's"],
                &["his", "owner's"],
                &["its", "controller's"],
                &["her", "controller's"],
                &["his", "controller's"],
                &["that", "creature's", "controller's"],
                &["that", "spell's", "controller's"],
            ],
            ZoneOwner::ItsOwner,
        ),
        (&[&["target", "opponent's"]], ZoneOwner::TargetOpponent),
        (&[&["target", "player's"]], ZoneOwner::TargetPlayer),
        (
            &[
                &["that", "player's"],
                &["the", "chosen", "player's"],
                &["their"],
            ],
            ZoneOwner::ThatPlayer,
        ),
        (
            &[
                &["an", "opponent's"],
                &["each", "opponent's"],
                &["that", "opponent's"],
                &["enchanted", "player's"],
                &["defending", "player's"],
            ],
            ZoneOwner::Opponent,
        ),
        (&[&["each", "player's"], &["each"]], ZoneOwner::EachPlayer),
        (&[&["your", "team's"], &["your"]], ZoneOwner::You),
        (&[&["all"]], ZoneOwner::All),
        (
            &[
                &["a", "single", "player's"],
                &["a", "player's"],
                &["any", "player's"],
                &["another", "player's"],
                &["a", "single"],
                &["any"],
                &["a"],
                &["that"],
            ],
            ZoneOwner::Any,
        ),
    ];
    let owner = eat_phrase_groups(ts, ZONE_OWNERS).unwrap_or(ZoneOwner::Default);

    let _ = ts.eat_word("opening");
    let zone = if ts.eat_any_word(&["graveyard", "graveyards"]) {
        ZoneKind::Graveyard
    } else if ts.eat_any_word(&["hand", "hands"]) {
        ZoneKind::Hand
    } else if ts.eat_any_phrase(&[&["library"], &["libraries"], &["starting", "deck"]]) {
        ZoneKind::Library
    } else {
        ts.restore(cp);
        return Err(ts.err("expected zone name"));
    };
    Ok(ZoneRef { zone, owner })
}

fn library_dest(
    owner: ZoneOwner,
    library_position: LibraryPosition,
    library_order: Option<LibraryOrder>,
) -> ZoneDest {
    ZoneDest {
        zone: ZoneKind::Library,
        owner,
        library_position: Some(library_position),
        library_order,
        under_control: None,
    }
}

/// Parse a typed zone destination (`into your hand`, `onto the battlefield tapped under their owners' control`,
/// `on the bottom of your library in a random order`, etc.) along with entry modifiers.
pub fn parse_zone_dest(ts: &mut TokenStream<'_>) -> PResult<(ZoneDest, EntryModifiers)> {
    let cp = ts.checkpoint();
    // Optional `back` + preposition: to / into / onto / on
    let _ = ts.eat_word("back");
    let _ = ts.eat_any_word(&["to", "into", "onto", "on"]);

    let mut modifiers = EntryModifiers::default();

    // Library position destinations: `their/its owner's choice of the top or bottom of <lib>`, `the top of <lib>`, `the bottom of <lib>`
    if ts.eat_any_phrase(&[
        &["their", "choice", "of", "the", "top", "or", "bottom", "of"],
        &["your", "choice", "of", "the", "top", "or", "bottom", "of"],
        &["the", "top", "or", "bottom", "of"],
    ]) && let Ok(zref) = try_parse(ts, parse_zone_ref)
        && zref.zone == ZoneKind::Library
    {
        return Ok((
            library_dest(zref.owner, LibraryPosition::TopOrBottom, None),
            modifiers,
        ));
    }
    if ts.eat_any_phrase(&[&["the", "top", "of"], &["top", "of"]]) {
        let zref = parse_zone_ref(ts)?;
        if zref.zone != ZoneKind::Library {
            ts.restore(cp);
            return Err(ts.err("expected library after `top of`"));
        }
        if ts.eat_any_phrase(&[&["or", "on", "the", "bottom"], &["or", "bottom"]]) {
            return Ok((
                library_dest(zref.owner, LibraryPosition::TopOrBottom, None),
                modifiers,
            ));
        }
        let order = parse_library_order(ts);
        return Ok((
            library_dest(zref.owner, LibraryPosition::Top, order),
            modifiers,
        ));
    }
    if ts.eat_any_phrase(&[&["the", "bottom", "of"], &["bottom", "of"]]) {
        let zref = parse_zone_ref(ts)?;
        if zref.zone != ZoneKind::Library {
            ts.restore(cp);
            return Err(ts.err("expected library after `bottom of`"));
        }
        let order = parse_library_order(ts);
        return Ok((
            library_dest(zref.owner, LibraryPosition::Bottom, order),
            modifiers,
        ));
    }
    if ts.eat_phrase(&["the", "bottom"]) {
        let order = parse_library_order(ts);
        return Ok((
            library_dest(ZoneOwner::Default, LibraryPosition::Bottom, order),
            modifiers,
        ));
    }

    let zref = parse_zone_ref(ts)?;
    let mut under_control = None;
    let mut library_position = None;
    if zref.zone == ZoneKind::Library {
        const FROM_TOP: &[(&[&str], u16)] = &[
            (&["second", "from", "the", "top"], 2),
            (&["third", "from", "the", "top"], 3),
            (&["fourth", "from", "the", "top"], 4),
        ];
        if let Some(depth) = eat_phrase_table(ts, FROM_TOP) {
            library_position = Some(LibraryPosition::FromTop(depth));
        } else {
            let jb_cp = ts.checkpoint();
            if ts.eat_phrase(&["just", "beneath", "the", "top"])
                && let Ok(ValueExpr::Const(n)) = try_parse(ts, crate::v2::value::parse_value_expr)
                && n >= 1
                && ts.eat_any_word(&["cards", "card"])
                && ts.eat_word("of")
                && try_parse(ts, parse_zone_ref).is_ok()
            {
                let d = u16::try_from(n).unwrap_or(1);
                library_position = Some(LibraryPosition::FromTop(d));
            } else {
                ts.restore(jb_cp);
            }
        }
        if library_position.is_some()
            && ts.eat_any_phrase(&[
                &["or", "on", "the", "bottom"],
                &["or", "on", "the", "bottom", "of", "their", "library"],
                &[
                    "or", "on", "the", "bottom", "of", "its", "owner's", "library",
                ],
            ])
        {
            library_position = Some(LibraryPosition::TopOrBottom);
        }
    }

    const UNDER_CONTROL: &[(&[&[&str]], ZoneOwner)] = &[
        (&[&["under", "your", "control"]], ZoneOwner::You),
        (
            &[
                &["under", "their", "owners'", "control"],
                &["under", "their", "owner's", "control"],
            ],
            ZoneOwner::TheirOwners,
        ),
        (
            &[
                &["under", "its", "owner's", "control"],
                &["under", "its", "controller's", "control"],
            ],
            ZoneOwner::ItsOwner,
        ),
        (
            &[&[
                "under",
                "the",
                "control",
                "of",
                "that",
                "creature's",
                "owner",
            ]],
            ZoneOwner::ThatCreatureOwner,
        ),
        (
            &[
                &["under", "that", "player's", "control"],
                &["under", "their", "control"],
            ],
            ZoneOwner::ThatPlayer,
        ),
        (
            &[
                &["under", "an", "opponent's", "control"],
                &["under", "that", "opponent's", "control"],
            ],
            ZoneOwner::Opponent,
        ),
        (
            &[&["under", "target", "opponent's", "control"]],
            ZoneOwner::TargetOpponent,
        ),
        (
            &[&["under", "target", "player's", "control"]],
            ZoneOwner::TargetPlayer,
        ),
        (
            &[&["under", "defending", "player's", "control"]],
            ZoneOwner::DefendingPlayer,
        ),
    ];

    // Parse trailing battlefield/exile modifiers in any order
    loop {
        if ts.eat_word("tapped") {
            modifiers.tapped = true;
            continue;
        }
        if ts.eat_any_phrase(&[&["and", "attacking"], &["attacking"]]) {
            modifiers.attacking = true;
            continue;
        }
        if ts.eat_any_phrase(&[&["face", "down"], &["face-down"]]) {
            modifiers.face_down = true;
            continue;
        }
        if ts.eat_any_phrase(&[&["face", "up"], &["face-up"]]) {
            modifiers.face_down = false;
            continue;
        }
        if ts.eat_phrase(&["attached", "to"])
            && let Ok(host) = try_parse(ts, parse_subject_ref)
        {
            modifiers.attached_to = Some(Box::new(host));
            continue;
        }
        if ts.eat_any_phrase(&[&["and", "transformed"], &["transformed"]]) {
            modifiers.transformed = true;
            continue;
        }
        if let Some(owner) = eat_phrase_groups(ts, UNDER_CONTROL) {
            under_control = Some(owner);
            continue;
        }
        if ts
            .peek_word()
            .is_some_and(|w| w.eq_ignore_ascii_case("with"))
        {
            let wcp = ts.checkpoint();
            ts.eat_word("with");
            if let Some(cnt) = ts.next_number(true)
                && cnt >= 1
            {
                let _ = ts.eat_word("additional");
                if let Ok(ck) = try_parse(ts, parse_counter_kind)
                    && (ts.eat_word("counter") || ts.eat_word("counters"))
                {
                    let _ = ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]);
                    modifiers.with_counter = Some((ck, u16::try_from(cnt).unwrap_or(1)));
                    continue;
                }
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

fn has_filter_head(
    flt: &ObjectFilter,
    card_types_seen: &[CardTypeMask],
    saw_subtype: bool,
) -> bool {
    flt.any_permanent
        || flt.any_card
        || flt.any_spell
        || flt.is_ability
        || !card_types_seen.is_empty()
        || saw_subtype
}

fn simple_filter_flag(low: &str) -> Option<FilterFlags> {
    match low {
        "another" | "non-~" => Some(FilterFlags::EXCLUDE_SELF),
        "kicked" => Some(FilterFlags::KICKED),
        "goaded" => Some(FilterFlags::GOADED),
        "suspected" => Some(FilterFlags::SUSPECTED),
        "warped" => Some(FilterFlags::WARPED),
        "suspended" => Some(FilterFlags::SUSPENDED),
        "stickered" => Some(FilterFlags::STICKERED),
        "renowned" => Some(FilterFlags::RENOWNED),
        "adventure" => Some(FilterFlags::HAS_ADVENTURE),
        "modified" => Some(FilterFlags::MODIFIED),
        "transformed" => Some(FilterFlags::TRANSFORMED),
        "equipped" => Some(FilterFlags::EQUIPPED),
        "face-down" => Some(FilterFlags::FACE_DOWN),
        "face-up" => Some(FilterFlags::FACE_UP),
        "commander" | "commanders" => Some(FilterFlags::IS_COMMANDER),
        "noncommander" => Some(FilterFlags::NOT_COMMANDER),
        _ => None,
    }
}

fn forbidden_type_word(low: &str) -> Option<CardTypeMask> {
    match low {
        "nonland" => Some(CardTypeMask::LAND),
        "noncreature" => Some(CardTypeMask::CREATURE),
        "nonartifact" => Some(CardTypeMask::ARTIFACT),
        "nonenchantment" => Some(CardTypeMask::ENCHANTMENT),
        "nonplaneswalker" => Some(CardTypeMask::PLANESWALKER),
        "nonbattle" => Some(CardTypeMask::BATTLE),
        "noninstant" => Some(CardTypeMask::INSTANT),
        "nonsorcery" => Some(CardTypeMask::SORCERY),
        "nonkindred" => Some(CardTypeMask::KINDRED),
        _ => None,
    }
}

fn forbidden_supertype_word(low: &str) -> Option<SupertypeMask> {
    match low {
        "nonbasic" => Some(SupertypeMask::BASIC),
        "nonlegendary" => Some(SupertypeMask::LEGENDARY),
        "nonsnow" => Some(SupertypeMask::SNOW),
        _ => None,
    }
}

fn forbidden_color_word(low: &str) -> Option<ColorMask> {
    match low {
        "nonwhite" => Some(ColorMask::WHITE),
        "nonblue" => Some(ColorMask::BLUE),
        "nonblack" => Some(ColorMask::BLACK),
        "nonred" => Some(ColorMask::RED),
        "nongreen" => Some(ColorMask::GREEN),
        _ => None,
    }
}

fn color_arity_word(low: &str) -> Option<ColorArity> {
    match low {
        "colorless" => Some(ColorArity::Colorless),
        "colored" => Some(ColorArity::Colored),
        "multicolored" => Some(ColorArity::Multicolored),
        "monocolored" => Some(ColorArity::Monocolored),
        _ => None,
    }
}

/// Strictly-typed noun-phrase [`ObjectFilter`] parser with explicit `all_types` (AND) vs
/// `any_types` (OR), typed stat bounds, `"power or toughness"` disjunction, `"differently named"`,
/// controller/owner, multi-zone sources, and relational relative clauses.
pub fn parse_object_filter(ts: &mut TokenStream<'_>) -> PResult<ObjectFilter> {
    let start_cp = ts.checkpoint();
    let mut flt = ObjectFilter::default();

    // Optional leading articles / `another` / `other` / `differently named`
    let _ = ts.eat_any_word(&["a", "an", "the"]);
    // Support self-named planeswalker subtype filter (`a ~ planeswalker`)
    {
        let pcp = ts.checkpoint();
        if ts.eat_tilde() && ts.eat_any_word(&["planeswalker", "planeswalkers"]) {
            flt.any_types = flt.any_types.union(CardTypeMask::PLANESWALKER);
            flt.predicates.push(FilterPredicate::NamedSelf);
            parse_filter_post_modifiers(ts, &mut flt)?;
            return Ok(flt);
        }
        ts.restore(pcp);
    }
    if ts.eat_any_phrase(&[&["your", "commanders"], &["your", "commander"]]) {
        flt.any_permanent = true;
        flt.controller = Some(crate::ast::ControllerRelation::You);
        flt.flags = flt.flags.union(FilterFlags::IS_COMMANDER);
        parse_filter_post_modifiers(ts, &mut flt)?;
        return Ok(flt);
    }
    if ts.eat_any_word(&["other", "another"]) {
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

    let loop_start = ts.pos;
    loop {
        let cp = ts.checkpoint();
        // Handle `non-~` (tokenized as `Word("non")`, `Minus`, `Tilde`, e.g. `non-~ creatures`)
        if ts.eat_word("non") && matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Minus)) && {
            ts.next_tok();
            ts.eat_tilde()
        } {
            flt.flags = flt.flags.union(FilterFlags::EXCLUDE_SELF);
            continue;
        }
        ts.restore(cp);
        // Handle `differently named` anywhere in modifier list
        if ts.eat_phrase(&["differently", "named"]) {
            flt.flags = flt.flags.union(FilterFlags::DIFFERENT_NAMES);
            continue;
        }
        // Handle comma between pre-nominal modifiers like `nonwhite, nonartifact` or `nonartifact, nonred`.
        // NEVER eat a leading comma before any modifier has been parsed, nor once a head noun/type/subtype has been parsed.
        if ts.pos > loop_start && ts.eat_comma() {
            if !has_filter_head(&flt, &card_types_seen, saw_subtype)
                && (ts.peek_word().is_some_and(|w| is_filter_modifier_word(w, ts.vocab))
                    || (ts.peek_word().is_some_and(|w| {
                        w.eq_ignore_ascii_case("or") || w.eq_ignore_ascii_case("and/or")
                    }) && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if is_filter_modifier_word(nw, ts.vocab))
                    })))
            {
                continue;
            }
            // Allow Oxford comma in spell/ability lists (`instant spell, sorcery spell, activated ability, or triggered ability`)
            if (flt.any_spell || flt.is_ability)
                && (ts.peek_word().is_some_and(|w| {
                    let low = w.to_ascii_lowercase();
                    matches!(low.as_str(), "activated" | "triggered")
                        || (lookup_card_type(&low).is_some()
                            && ts.peek_n(1).is_some_and(|t| {
                                matches!(t.kind, TokenKind::Word(nw) if nw.eq_ignore_ascii_case("spell") || nw.eq_ignore_ascii_case("spells"))
                            }))
                }) || (ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("or") || w.eq_ignore_ascii_case("and/or")
                }) && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if {
                        let nl = nw.to_ascii_lowercase();
                        matches!(nl.as_str(), "activated" | "triggered" | "ability" | "abilities")
                            || lookup_card_type(&nl).is_some()
                    })
                })))
            {
                pending_or_type = true;
                continue;
            }
            // Allow Oxford comma inside card-type + subtype lists (`artifact, creature, or enchantment`, `artifacts, enchantments, and/or tokens`)
            if !card_types_seen.is_empty() {
                if ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("or")
                        || w.eq_ignore_ascii_case("and")
                        || w.eq_ignore_ascii_case("and/or")
                }) && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if {
                        let nl = nw.to_ascii_lowercase();
                        is_filter_modifier_word(nw, ts.vocab)
                            || matches!(
                                nl.as_str(),
                                "token"
                                    | "tokens"
                                    | "card"
                                    | "cards"
                                    | "spell"
                                    | "spells"
                                    | "permanent"
                                    | "permanents"
                                    | "ability"
                                    | "abilities"
                                    | "activated"
                                    | "triggered"
                            )
                    })
                }) {
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
            // Allow Oxford comma inside subtype lists (`Elf, Warrior, or Tyvar`, `Knight, Aura, Equipment, or legendary artifact`)
            if saw_subtype {
                if ts.peek_word().is_some_and(|w| {
                    w.eq_ignore_ascii_case("or")
                        || w.eq_ignore_ascii_case("and")
                        || w.eq_ignore_ascii_case("and/or")
                }) && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if {
                        let nl = nw.to_ascii_lowercase();
                        is_filter_modifier_word(nw, ts.vocab)
                            || matches!(
                                nl.as_str(),
                                "token"
                                    | "tokens"
                                    | "card"
                                    | "cards"
                                    | "spell"
                                    | "spells"
                                    | "permanent"
                                    | "permanents"
                            )
                    })
                }) {
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
            ts.next_tok();
            push_stat_bound(
                &mut flt,
                StatKind::Power,
                CompareOp::Eq,
                ValueExpr::Const(p),
            );
            push_stat_bound(
                &mut flt,
                StatKind::Toughness,
                CompareOp::Eq,
                ValueExpr::Const(t),
            );
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
                    is_filter_modifier_word(nw, ts.vocab)
                        || matches!(
                            nl.as_str(),
                            "a" | "an"
                                | "activated"
                                | "triggered"
                                | "ability"
                                | "abilities"
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
                let _ = ts.eat_any_word(&["a", "an"]);
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
            // Allow `activated and triggered abilities`
            if flt.is_ability
                && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if nw.eq_ignore_ascii_case("triggered") || nw.eq_ignore_ascii_case("activated"))
                })
            {
                ts.next_word();
                continue;
            }
            // Allow `spell and ability` / `spells and abilities`
            if flt.any_spell
                && ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if nw.eq_ignore_ascii_case("ability") || nw.eq_ignore_ascii_case("abilities"))
                })
            {
                ts.next_word();
                ts.next_word();
                flt.is_ability = true;
                continue;
            }
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
            "exiled" => {
                // Leave post-nominal `cards exiled this way` / `cards exiled with ~` for `parse_filter_post_modifiers`.
                if has_filter_head(&flt, &card_types_seen, saw_subtype) {
                    break;
                }
                ts.next_word();
                flt.predicates.push(FilterPredicate::AffectedByPriorAction(
                    crate::v2::ir::PriorActionKind::ExiledThisWay,
                ));
            }
            "permanent" | "permanents" | "source" | "sources" => {
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
            "ability" | "abilities" | "activated" | "triggered" => {
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
                if has_filter_head(&flt, &card_types_seen, saw_subtype) {
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
                if has_filter_head(&flt, &card_types_seen, saw_subtype) {
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
                if has_filter_head(&flt, &card_types_seen, saw_subtype) {
                    break;
                }
                ts.next_word();
                flt.combat = Some(CombatState::Blocked);
            }
            "unblocked" => {
                ts.next_word();
                flt.combat = Some(CombatState::Unblocked);
            }
            "last" => {
                let next_is_obj = ts.peek_n(1).is_some_and(|t| {
                    matches!(t.kind, TokenKind::Word(nw) if {
                        let nl = nw.to_ascii_lowercase();
                        is_filter_modifier_word(nw, ts.vocab)
                            || matches!(nl.as_str(), "card" | "cards" | "spell" | "spells" | "permanent" | "permanents")
                    })
                });
                if !has_filter_head(&flt, &card_types_seen, saw_subtype) && next_is_obj {
                    ts.next_word();
                    flt.flags = flt.flags.union(FilterFlags::LKI_PAST_TENSE);
                } else {
                    break;
                }
            }
            "enchanted" => {
                // Leave `a creature enchanted player controls` for `parse_filter_post_modifiers`!
                if has_filter_head(&flt, &card_types_seen, saw_subtype)
                    && ts.peek_n(1).is_some_and(
                        |t| matches!(t.kind, TokenKind::Word(w) if w.eq_ignore_ascii_case("player")),
                    )
                {
                    break;
                }
                ts.next_word();
                flt.flags = flt.flags.union(FilterFlags::ENCHANTED);
            }
            "nonlands" => {
                ts.next_word();
                flt.forbidden_types = flt.forbidden_types.union(CardTypeMask::LAND);
                flt.any_permanent = true;
            }
            "nonattacking" => {
                ts.next_word();
                flt.combat = Some(CombatState::NotAttacking);
            }
            "nonblocking" => {
                ts.next_word();
                push_exclude_filter(
                    &mut flt,
                    ObjectFilter {
                        combat: Some(CombatState::Blocking),
                        ..Default::default()
                    },
                );
            }
            _ => {
                if let Some(flag) = simple_filter_flag(&low) {
                    ts.next_word();
                    flt.flags = flt.flags.union(flag);
                } else if let Some(arity) = color_arity_word(&low) {
                    ts.next_word();
                    flt.color_arity = Some(arity);
                } else if let Some(ft) = forbidden_type_word(&low) {
                    ts.next_word();
                    flt.forbidden_types = flt.forbidden_types.union(ft);
                } else if let Some(fs) = forbidden_supertype_word(&low) {
                    ts.next_word();
                    flt.forbidden_supertypes = flt.forbidden_supertypes.union(fs);
                } else if let Some(fc) = forbidden_color_word(&low) {
                    ts.next_word();
                    flt.forbidden_colors = flt.forbidden_colors.union(fc);
                } else if let Some(c) = lookup_color(&low) {
                    ts.next_word();
                    flt.any_colors = flt.any_colors.union(c);
                    // Support comma/or color lists (`red, white, or black creature card`)
                    loop {
                        let ccp = ts.checkpoint();
                        let had_comma = ts.eat_comma();
                        let had_conj = ts.eat_any_word(&["or", "and/or"]);
                        if (had_comma || had_conj)
                            && let Some(nw) = ts.peek_word()
                            && let Some(nc) = lookup_color(&nw.to_ascii_lowercase())
                        {
                            ts.next_word();
                            flt.any_colors = flt.any_colors.union(nc);
                            continue;
                        }
                        ts.restore(ccp);
                        break;
                    }
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
        || low
            .strip_prefix("non-")
            .is_some_and(|rest| vocab.lookup_subtype(rest).is_some())
        || matches!(
            low.as_str(),
            "nonwhite"
                | "nonblue"
                | "nonblack"
                | "nonred"
                | "nongreen"
                | "nonland"
                | "nonlands"
                | "noncreature"
                | "nonartifact"
                | "nonenchantment"
                | "nonplaneswalker"
                | "nonbattle"
                | "noninstant"
                | "nonsorcery"
                | "nonkindred"
                | "nonbasic"
                | "nonlegendary"
                | "nonsnow"
                | "nontoken"
                | "noncommander"
                | "nonattacking"
                | "nonblocking"
                | "tapped"
                | "untapped"
                | "nontapped"
                | "attacking"
                | "blocking"
                | "blocked"
                | "unblocked"
                | "modified"
                | "transformed"
                | "equipped"
                | "enchanted"
                | "face-down"
                | "face-up"
                | "colorless"
                | "colored"
                | "multicolored"
                | "monocolored"
        )
}
