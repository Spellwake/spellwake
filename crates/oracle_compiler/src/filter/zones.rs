//! Compositional zone-suffix and controller/owner-suffix parsers for `ObjectFilterAst` and
//! `SubjectRefAst`.
//!
//! Replaces the old Cartesian-product `ZONE_SUFFIXES` (440 LOC) and `CTRL_SUFFIXES` (250 LOC)
//! lookup tables with structured grammatical decomposition over prepositions (`from`/`in`/`on`),
//! multi-zone conjunctions (`and/or`, `or`, `and`), player possessives, and control/ownership
//! predicates — generalizing cleanly to novel custom-card wordings.

use crate::ast::{ControllerRelation, ZoneFilter};

/// Peel a trailing zone phrase (e.g. `" from your graveyard"`, `" in exile or your graveyard"`,
/// `" from your hand, graveyard, and/or library"`) from `s`, returning `(prefix, ZoneFilter)`.
///
/// Handles arbitrary preposition (`from`/`in`/`on`), multi-zone disjunction/conjunction ordering,
/// and possessive player qualifiers compositionally instead of enumerating Cartesian permutations.
pub(crate) fn peel_zone_suffix(s: &str) -> Option<(&str, ZoneFilter)> {
    // 1. Special idiomatic / prior-action / multi-clause zone tails checked longest-first.
    const SPECIAL_ZONE_TAILS: &[(&str, ZoneFilter)] = &[
        (
            " that were put into your graveyard from your library this turn",
            ZoneFilter::YourGraveyard,
        ),
        (
            " with the greatest power among creatures on the battlefield",
            ZoneFilter::Battlefield,
        ),
        (
            " from among the nonland cards exiled this way",
            ZoneFilter::Exile,
        ),
        (
            " from among the cards milled this way",
            ZoneFilter::YourGraveyard,
        ),
        (
            " from among cards revealed this way",
            ZoneFilter::YourLibrary,
        ),
        (" from among cards exiled this way", ZoneFilter::Exile),
        (" from among cards exiled with ~", ZoneFilter::Exile),
        (" from among the milled cards", ZoneFilter::YourGraveyard),
        (" from among the exiled cards", ZoneFilter::Exile),
        (" from among those nonland cards", ZoneFilter::Exile),
        (" from among those cards", ZoneFilter::YourLibrary),
        (" from among them", ZoneFilter::YourLibrary),
        (
            " you own in exile and in your graveyard",
            ZoneFilter::YourGraveyardOrExile,
        ),
        (" your opponents own in exile", ZoneFilter::Exile),
        (" an opponent owns in exile", ZoneFilter::Exile),
        (" you own in exile", ZoneFilter::Exile),
        (" you cast from your graveyard", ZoneFilter::YourGraveyard),
        (" you cast from exile", ZoneFilter::Exile),
        (" cast from your graveyard", ZoneFilter::YourGraveyard),
        (" cast from any graveyard", ZoneFilter::AnyGraveyard),
        (" cast from a graveyard", ZoneFilter::AnyGraveyard),
        (" cast from exile", ZoneFilter::Exile),
        (" from outside the game", ZoneFilter::OutsideTheGame),
        (" outside the game", ZoneFilter::OutsideTheGame),
        (" from it", ZoneFilter::YourHand),
        (" in it", ZoneFilter::YourHand),
    ];
    for (suf, z) in SPECIAL_ZONE_TAILS {
        if let Some(prefix) = s.strip_suffix(suf) {
            return Some((prefix.trim(), *z));
        }
    }

    // 2. Compositional preposition + zone expression: scan from leftmost candidate " from " / " in " / " on "
    //    (choosing the earliest split whose tail parses as a complete zone expression so multi-zone
    //    tails like " from your graveyard or from exile" match in full rather than only " from exile").
    let mut best: Option<(usize, ZoneFilter)> = None;
    for prep in [" from ", " in ", " on "] {
        let mut offset = 0;
        while let Some(rel_pos) = s[offset..].find(prep) {
            let abs_pos = offset + rel_pos;
            let tail = &s[abs_pos + prep.len()..];
            let is_on = prep == " on ";
            if let Some(zone) = parse_zone_expr(tail, is_on) {
                match best {
                    Some((best_pos, _)) if abs_pos >= best_pos => {}
                    _ => best = Some((abs_pos, zone)),
                }
                break;
            }
            offset = abs_pos + 1;
        }
    }
    best.map(|(pos, z)| (s[..pos].trim(), z))
}

/// Parse a bare zone expression (after stripping leading `from ` / `in ` / `on `), supporting
/// 1-, 2-, and 3-way multi-zone combinations in any order.
fn parse_zone_expr(expr: &str, require_surface_zone: bool) -> Option<ZoneFilter> {
    let e = expr.trim();
    if require_surface_zone {
        return match e {
            "the battlefield" => Some(ZoneFilter::Battlefield),
            "the stack" => Some(ZoneFilter::Stack),
            _ => None,
        };
    }

    // Check multi-zone combinations first (normalize connectors & repeated prepositions).
    if (e.contains(" or ") || e.contains(" and/or ") || e.contains(" and "))
        && let Some(multi) = parse_multi_zone_expr(e)
    {
        return Some(multi);
    }

    parse_single_zone_atom(e)
}

/// Parse a 2- or 3-zone combination like `"your graveyard, hand, and/or library"`,
/// `"your graveyard or from exile"`, `"exile or any graveyard"`, `"your hand and/or graveyard"`.
fn parse_multi_zone_expr(expr: &str) -> Option<ZoneFilter> {
    // Split on ", and/or ", ", or ", " and/or ", " or ", " and ", ", "
    let norm = expr
        .replace(", and/or ", "|")
        .replace(", or ", "|")
        .replace(" and/or ", "|")
        .replace(" or ", "|")
        .replace(" and ", "|")
        .replace(", ", "|");
    let raw_parts: Vec<&str> = norm
        .split('|')
        .map(|p| {
            let p = p.trim();
            p.strip_prefix("from ")
                .or_else(|| p.strip_prefix("in "))
                .unwrap_or(p)
                .trim()
        })
        .collect();
    if raw_parts.len() < 2 || raw_parts.len() > 3 {
        return None;
    }

    // Track shared leading owner ("your graveyard, hand, and/or library" -> owner "your")
    let shared_your = raw_parts[0].starts_with("your ");
    let mut has_your_gy = false;
    let mut has_any_gy = false;
    let mut has_your_hand = false;
    let mut has_your_lib = false;
    let mut has_exile = false;

    for part in raw_parts {
        let p = if shared_your && matches!(part, "graveyard" | "hand" | "library") {
            match part {
                "graveyard" => "your graveyard",
                "hand" => "your hand",
                "library" => "your library",
                _ => part,
            }
        } else {
            part
        };
        match p {
            "your graveyard" => has_your_gy = true,
            "any graveyard" | "a graveyard" => has_any_gy = true,
            "your hand" => has_your_hand = true,
            "your library" => has_your_lib = true,
            "exile" => has_exile = true,
            _ => return None,
        }
    }

    match (
        has_your_gy,
        has_any_gy,
        has_your_hand,
        has_your_lib,
        has_exile,
    ) {
        (true, false, true, true, false) => Some(ZoneFilter::YourGraveyardHandOrLibrary),
        (true, false, false, true, false) => Some(ZoneFilter::YourLibraryOrGraveyard),
        (true, false, false, false, true) => Some(ZoneFilter::YourGraveyardOrExile),
        (false, true, false, false, true) => Some(ZoneFilter::AnyGraveyardOrExile),
        (true, false, true, false, false) => Some(ZoneFilter::YourHandOrGraveyard),
        _ => None,
    }
}

/// Parse one single-zone phrase (without leading preposition).
fn parse_single_zone_atom(e: &str) -> Option<ZoneFilter> {
    match e {
        "exile" => return Some(ZoneFilter::Exile),
        "the command zone" => return Some(ZoneFilter::CommandZone),
        "the battlefield" => return Some(ZoneFilter::Battlefield),
        "the stack" => return Some(ZoneFilter::Stack),
        "graveyards" | "all graveyards" => return Some(ZoneFilter::AllGraveyards),
        "hands" | "all hands" => return Some(ZoneFilter::AnyHand),
        _ => {}
    }

    // Factor possessive owner × zone noun (`graveyard(s)`, `hand(s)`, `library`/`libraries`)
    if let Some(owner) = e
        .strip_suffix(" graveyards")
        .or_else(|| e.strip_suffix(" graveyard"))
    {
        return match owner {
            "target opponent's" => Some(ZoneFilter::TargetOpponentGraveyard),
            "your opponents'" | "each opponent's" | "opponents'" => {
                Some(ZoneFilter::OpponentsGraveyards)
            }
            "an opponent's" | "that opponent's" => Some(ZoneFilter::OpponentGraveyard),
            "that player's" | "the chosen player's" | "their" | "that" | "its controller's" => {
                Some(ZoneFilter::ThatPlayerGraveyard)
            }
            "target player's" => Some(ZoneFilter::TargetPlayerGraveyard),
            "defending player's" | "the defending player's" => {
                Some(ZoneFilter::DefendingPlayerGraveyard)
            }
            "a single" => Some(ZoneFilter::SingleGraveyard),
            "all players'" | "each player's" => Some(ZoneFilter::AllGraveyards),
            "your" => Some(ZoneFilter::YourGraveyard),
            "any player's" | "a player's" | "any" | "a" => Some(ZoneFilter::AnyGraveyard),
            "its owner's" => Some(ZoneFilter::ItsOwnerGraveyard),
            _ => None,
        };
    }

    if let Some(owner) = e.strip_suffix(" hands").or_else(|| e.strip_suffix(" hand")) {
        return match owner {
            "target opponent's" => Some(ZoneFilter::TargetOpponentHand),
            "target player's" => Some(ZoneFilter::TargetPlayerHand),
            "an opponent's" | "that opponent's" => Some(ZoneFilter::OpponentHand),
            "your opponents'" | "opponents'" | "each opponent's" => {
                Some(ZoneFilter::OpponentsHands)
            }
            "each player's" | "all players'" => Some(ZoneFilter::EachPlayerHand),
            "the chosen player's" | "that player's" | "their" => Some(ZoneFilter::ThatPlayerHand),
            "your" => Some(ZoneFilter::YourHand),
            "a player's" | "any player's" | "a" | "any" => Some(ZoneFilter::AnyHand),
            _ => None,
        };
    }

    if let Some(owner) = e.strip_suffix(" library") {
        return match owner {
            "target opponent's" => Some(ZoneFilter::TargetOpponentLibrary),
            "target player's" => Some(ZoneFilter::TargetPlayerLibrary),
            "an opponent's" | "that opponent's" => Some(ZoneFilter::OpponentLibrary),
            "the chosen player's" | "that player's" | "their" => {
                Some(ZoneFilter::ThatPlayerLibrary)
            }
            "each player's" => Some(ZoneFilter::EachPlayerLibrary),
            "your" => Some(ZoneFilter::YourLibrary),
            "a" | "any" => Some(ZoneFilter::AnyLibrary),
            _ => None,
        };
    }

    None
}

/// Result of peeling a trailing controller or owner phrase from an object filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControllerSuffixMatch<'a> {
    pub prefix: &'a str,
    pub relation: ControllerRelation,
    pub is_past_tense: bool,
}

/// Compositional controller/owner suffix parser replacing the 250-line `CTRL_SUFFIXES` table.
///
/// Decomposes:
/// - Special multi-clause control/ownership suffixes (`you own but don't control`, `you cast from exile`, etc.)
/// - `" controlled by <who>"`, `" under <who>'s control"`, `" owned by <who>"`
/// - `" <who> (controls|controlled|owns|owned|casts|cast|protects|protected|don't control|didn't control|don't own|didn't own)"`
pub(crate) fn peel_controller_suffix(s: &str) -> Option<ControllerSuffixMatch<'_>> {
    // 1. Multi-clause or compound idioms checked longest-first.
    const SPECIAL_CTRL_TAILS: &[(&str, ControllerRelation, bool)] = &[
        (
            " controlled by that player or by that planeswalker's controller",
            ControllerRelation::ThatPlayerOrPlaneswalkerController,
            false,
        ),
        (
            " that player or that planeswalker's controller controls",
            ControllerRelation::ThatPlayerOrPlaneswalkerController,
            false,
        ),
        (
            " that opponent or that planeswalker's controller controls",
            ControllerRelation::ThatPlayerOrPlaneswalkerController,
            false,
        ),
        (
            " controlled by the next player in the chosen direction",
            ControllerRelation::NextPlayerInDirection,
            false,
        ),
        (
            " controlled by the player to their left",
            ControllerRelation::PlayerToLeft,
            false,
        ),
        (
            " controlled by the player to their right",
            ControllerRelation::PlayerToRight,
            false,
        ),
        (
            " you cast from anywhere other than your hand",
            ControllerRelation::You,
            false,
        ),
        (
            " you own that aren't on the battlefield",
            ControllerRelation::OwnerYou,
            false,
        ),
        (
            " you own that isn't on the battlefield",
            ControllerRelation::OwnerYou,
            false,
        ),
        (
            " your opponents don't control",
            ControllerRelation::You,
            false,
        ),
        (
            " you both owned and controlled",
            ControllerRelation::You,
            true,
        ),
        (" you both own and control", ControllerRelation::You, false),
        (
            " you controlled but didn't own",
            ControllerRelation::You,
            true,
        ),
        (" you control but don't own", ControllerRelation::You, false),
        (
            " you owned but didn't control",
            ControllerRelation::OwnerYou,
            true,
        ),
        (
            " you own but don't control",
            ControllerRelation::OwnerYou,
            false,
        ),
        (
            " you cast from your graveyard",
            ControllerRelation::You,
            false,
        ),
        (" you cast but don't own", ControllerRelation::You, false),
        (" you cast from exile", ControllerRelation::You, false),
        (" you owned or controlled", ControllerRelation::You, true),
        (" you own or control", ControllerRelation::You, false),
        (" you didn't control", ControllerRelation::NotYou, true),
        (" you don't control", ControllerRelation::NotYou, false),
        (" you didn't own", ControllerRelation::OwnerOpponent, true),
        (" you don't own", ControllerRelation::OwnerOpponent, false),
    ];
    for (suf, rel, past) in SPECIAL_CTRL_TAILS {
        if let Some(prefix) = s.strip_suffix(suf) {
            return Some(ControllerSuffixMatch {
                prefix: prefix.trim(),
                relation: *rel,
                is_past_tense: *past,
            });
        }
    }

    // 2. Passive prepositional forms: `" controlled by <who>"`, `" owned by <who>"`, `" under <poss> control"`.
    if let Some((prefix, who)) = s.rsplit_once(" controlled by ")
        && let Some(rel) = map_controller_who(who.trim(), false)
    {
        return Some(ControllerSuffixMatch {
            prefix: prefix.trim(),
            relation: rel,
            is_past_tense: false,
        });
    }
    if let Some((prefix, who)) = s.rsplit_once(" owned by ")
        && let Some(rel) = map_controller_who(who.trim(), true)
    {
        return Some(ControllerSuffixMatch {
            prefix: prefix.trim(),
            relation: rel,
            is_past_tense: false,
        });
    }
    if let Some(before_ctrl) = s.strip_suffix(" control")
        && let Some((prefix, poss)) = before_ctrl.rsplit_once(" under ")
        && let Some(rel) = map_possessive_controller(poss.trim())
    {
        return Some(ControllerSuffixMatch {
            prefix: prefix.trim(),
            relation: rel,
            is_past_tense: false,
        });
    }

    // 3. Active relative clauses: `" <who> <verb>"` where `<verb>` is control/own/cast/protect (present or past).
    const ACTIVE_VERBS: &[(&str, bool, bool)] = &[
        // (suffix, is_owner_verb, is_past_tense)
        (" controlled", false, true),
        (" controls", false, false),
        (" control", false, false),
        (" protected", false, true),
        (" protects", false, false),
        (" protect", false, false),
        (" owned", true, true),
        (" owns", true, false),
        (" own", true, false),
        (" casts", false, false),
        (" cast", false, false),
    ];
    for (verb_suf, is_owner, is_past) in ACTIVE_VERBS {
        let Some(before_verb) = s.strip_suffix(verb_suf) else {
            continue;
        };
        // Guard: preserve legacy behavior where `" you own"` isn't peeled off `" attached to ... you own"`.
        if *verb_suf == " own" && before_verb.ends_with(" you") && s.contains(" attached to ") {
            continue;
        }
        // Match longest active subject tail before the verb.
        const SUBJECT_TAILS: &[(&str, ControllerRelation, Option<ControllerRelation>)] = &[
            (
                " the defending player",
                ControllerRelation::DefendingPlayer,
                None,
            ),
            (
                " defending player",
                ControllerRelation::DefendingPlayer,
                None,
            ),
            (" the active player", ControllerRelation::ActivePlayer, None),
            (
                " the chosen player",
                ControllerRelation::ThatPlayer,
                Some(ControllerRelation::OwnerThatPlayer),
            ),
            (" enchanted player", ControllerRelation::ThatPlayer, None),
            (" target opponent", ControllerRelation::TargetOpponent, None),
            (
                " target player",
                ControllerRelation::TargetPlayer,
                Some(ControllerRelation::OwnerThatPlayer),
            ),
            (
                " your opponents",
                ControllerRelation::Opponents,
                Some(ControllerRelation::OwnerOpponent),
            ),
            (
                " each opponent",
                ControllerRelation::Opponents,
                Some(ControllerRelation::OwnerOpponent),
            ),
            (" that opponent", ControllerRelation::Opponent, None),
            (
                " an opponent",
                ControllerRelation::Opponent,
                Some(ControllerRelation::OwnerOpponent),
            ),
            (" its controller", ControllerRelation::ItsController, None),
            (
                " that player",
                ControllerRelation::ThatPlayer,
                Some(ControllerRelation::OwnerThatPlayer),
            ),
            (" those players", ControllerRelation::ThatPlayer, None),
            (" your team", ControllerRelation::YourTeam, None),
            (
                " opponents",
                ControllerRelation::Opponents,
                Some(ControllerRelation::OwnerOpponent),
            ),
            (
                " they",
                ControllerRelation::ThatPlayer,
                Some(ControllerRelation::OwnerThatPlayer),
            ),
            (
                " you",
                ControllerRelation::You,
                Some(ControllerRelation::OwnerYou),
            ),
            (" players", ControllerRelation::ThatPlayer, None),
        ];
        for (subj_suf, ctrl_rel, owner_rel) in SUBJECT_TAILS {
            if let Some(prefix) = before_verb.strip_suffix(subj_suf) {
                let relation = if *is_owner { (*owner_rel)? } else { *ctrl_rel };
                return Some(ControllerSuffixMatch {
                    prefix: prefix.trim(),
                    relation,
                    is_past_tense: *is_past,
                });
            }
        }
    }

    None
}

fn map_controller_who(who: &str, is_owner: bool) -> Option<ControllerRelation> {
    if is_owner {
        return match who {
            "your opponents" | "an opponent" | "opponents" => {
                Some(ControllerRelation::OwnerOpponent)
            }
            "target player" | "that player" | "them" => Some(ControllerRelation::OwnerThatPlayer),
            "you" => Some(ControllerRelation::OwnerYou),
            _ => None,
        };
    }
    match who {
        "the defending player" | "defending player" => Some(ControllerRelation::DefendingPlayer),
        "target opponent" => Some(ControllerRelation::TargetOpponent),
        "target player" => Some(ControllerRelation::TargetPlayer),
        "your opponents" | "opponents" => Some(ControllerRelation::Opponents),
        "an opponent" | "that opponent" => Some(ControllerRelation::Opponent),
        "that player" | "them" => Some(ControllerRelation::ThatPlayer),
        "its controller" => Some(ControllerRelation::ItsController),
        "your team" => Some(ControllerRelation::YourTeam),
        "you" => Some(ControllerRelation::You),
        _ => None,
    }
}

fn map_possessive_controller(poss: &str) -> Option<ControllerRelation> {
    match poss {
        "defending player's" => Some(ControllerRelation::DefendingPlayer),
        "target opponent's" => Some(ControllerRelation::TargetOpponent),
        "target player's" => Some(ControllerRelation::TargetPlayer),
        "your opponents'" => Some(ControllerRelation::Opponents),
        "an opponent's" => Some(ControllerRelation::Opponent),
        "that player's" | "their" => Some(ControllerRelation::ThatPlayer),
        "its controller's" => Some(ControllerRelation::ItsController),
        "your" => Some(ControllerRelation::You),
        _ => None,
    }
}
