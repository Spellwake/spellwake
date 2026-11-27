//! Additive, strongly-typed IR decoders for raw `String` escape-hatch fields on
//! [`InstructionAst`] and [`StaticAbilityAst`].
//!
//! During Stage 1, existing `ast.rs` `String` fields remain intact for zero-conflict
//! backward compatibility with `spellwake-card-programs` and `spellwake-engine`.
//! Downstream lowering can opt into these typed decoders and accessor methods
//! (`InstructionAst::parsed_zone_dest`, `InstructionAst::parsed_restriction`,
//! `InstructionAst::parsed_combat_rule`, `InstructionAst::parsed_choice_option`,
//! `InstructionAst::parsed_duration_text`, `InstructionAst::parsed_grant_payload`,
//! `InstructionAst::parsed_mana_spend_restriction`, and their [`StaticAbilityAst`]
//! counterparts) instead of re-parsing English prose a second time.

use serde::{Deserialize, Serialize};

use crate::ast::{
    AbilityAst, CostAtomAst, DurationAst, FaceAst, InstructionAst, ObjectFilterAst, PlayerRefAst,
    ReplacementEffectAst, StaticAbilityAst, TriggerSpecAst, TurnStepAst,
};
use crate::clauses::{StaticOrReplacement, parse_static_or_replacement};
use crate::compile_granted_ability;
use crate::filter::{parse_condition, parse_keyword_list, parse_object_filter};
use crate::vocab::{Vocabulary, parse_number_word};

// ---------------------------------------------------------------------------
// 1. ZoneDestAst
// ---------------------------------------------------------------------------

/// Typed destination zone category for zone-move and library-search instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKindAst {
    Battlefield,
    Hand,
    Graveyard,
    Library,
    Exile,
    CommandZone,
    Stack,
    /// Object stays in its current zone (`stay_in:<zone>`).
    StayIn(Box<ZoneKindAst>),
    /// Intermediate found/revealed/follow-up set handoff inside a multi-step effect.
    FoundSet,
    Revealed,
    FollowUp,
    /// Disjunctive destination choice (e.g. `"the battlefield or into your hand"`).
    Choice(Vec<ZoneDestAst>),
}

/// Typed owner or controller qualifier for a destination zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneOwnerAst {
    Default,
    You,
    ItsOwner,
    TheirOwners,
    ThatPlayer,
    EachPlayer,
    Opponent,
    ThatObjectOwner,
}

/// Typed position inside a library destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryPositionAst {
    Top,
    Bottom,
    Shuffled,
    TopAfterShuffle,
    /// 1-based ordinal from top (`second from the top` -> `2`).
    FromTop(u16),
    /// Placed immediately beneath the top N cards.
    BeneathTop(u16),
    /// Owner/controller chooses top or bottom of library.
    TopOrBottomChoice,
    /// Owner chooses `N`th from top or bottom (`second from the top or on the bottom`).
    FromTopOrBottomChoice(u16),
}

/// Typed ordering rule when multiple cards enter a library together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryOrderAst {
    AnyOrder,
    RandomOrder,
}

/// Typed entry modifiers for cards moving onto the battlefield or into exile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryModifierAst {
    pub tapped: bool,
    pub attacking: bool,
    pub attacking_target: Option<String>,
    pub blocking: bool,
    pub face_down: bool,
    pub face_up: bool,
    pub face_up_or_down_choice: bool,
    pub transformed: bool,
    pub flipped: bool,
    pub random: bool,
    pub under_control: Option<ZoneOwnerAst>,
}

/// Strongly-typed zone destination decoded from `MoveZone::to`, `ReturnZone::to`,
/// `MoveCounted::to`, `SearchLibrary::to_zone`, or `ChooseAndKeep` destinations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneDestAst {
    pub zone: ZoneKindAst,
    pub owner: ZoneOwnerAst,
    pub library_position: Option<LibraryPositionAst>,
    pub library_order: Option<LibraryOrderAst>,
    pub modifiers: EntryModifierAst,
}

/// Decode a raw destination string (such as `"the battlefield tapped under your control"`,
/// `"the bottom of its owner's library in a random order"`, or `"your hand"`) into a
/// strongly-typed [`ZoneDestAst`].
///
/// Returns `None` if the string does not match any recognized Magic destination structure.
#[must_use]
pub fn parse_zone_dest(raw: &str) -> Option<ZoneDestAst> {
    let norm = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if norm.is_empty() {
        return None;
    }

    // Internal handoff tokens used by SearchLibrary / MoveZone / ChooseAndKeep.
    match norm.as_str() {
        "found_set" => {
            return Some(ZoneDestAst::simple(
                ZoneKindAst::FoundSet,
                ZoneOwnerAst::Default,
            ));
        }
        "revealed" => {
            return Some(ZoneDestAst::simple(
                ZoneKindAst::Revealed,
                ZoneOwnerAst::Default,
            ));
        }
        "follow_up" => {
            return Some(ZoneDestAst::simple(
                ZoneKindAst::FollowUp,
                ZoneOwnerAst::Default,
            ));
        }
        "top_of_library_after_shuffle" => {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner: ZoneOwnerAst::You,
                library_position: Some(LibraryPositionAst::TopAfterShuffle),
                library_order: None,
                modifiers: EntryModifierAst::default(),
            });
        }
        "owner_choice_top_or_bottom_of_library" => {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner: ZoneOwnerAst::ItsOwner,
                library_position: Some(LibraryPositionAst::TopOrBottomChoice),
                library_order: None,
                modifiers: EntryModifierAst::default(),
            });
        }
        "exile_random" => {
            let mut d = ZoneDestAst::simple(ZoneKindAst::Exile, ZoneOwnerAst::Default);
            d.modifiers.random = true;
            return Some(d);
        }
        _ => {}
    }

    if let Some(stay_rest) = norm.strip_prefix("stay_in:") {
        let inner = parse_zone_dest(stay_rest)?;
        return Some(ZoneDestAst {
            zone: ZoneKindAst::StayIn(Box::new(inner.zone)),
            owner: inner.owner,
            library_position: inner.library_position,
            library_order: inner.library_order,
            modifiers: inner.modifiers,
        });
    }

    // Strip optional leading preposition ("into ", "onto ", "on ", "to ").
    let s = norm
        .strip_prefix("into ")
        .or_else(|| norm.strip_prefix("onto "))
        .or_else(|| norm.strip_prefix("on "))
        .or_else(|| norm.strip_prefix("to "))
        .unwrap_or(&norm)
        .trim();

    // Disjunctive destination choices ("the battlefield or into your hand").
    if let Some((lhs, rhs)) = s
        .split_once(" or into ")
        .or_else(|| s.split_once(" or onto "))
        .or_else(|| s.split_once(" or on "))
        && let (Some(d1), Some(d2)) = (parse_zone_dest(lhs), parse_zone_dest(rhs))
    {
        return Some(ZoneDestAst {
            zone: ZoneKindAst::Choice(vec![d1, d2]),
            owner: ZoneOwnerAst::Default,
            library_position: None,
            library_order: None,
            modifiers: EntryModifierAst::default(),
        });
    }

    // Simple exile / command zone / stack destinations.
    if s == "exile" || s.starts_with("exile ") {
        let mut d = ZoneDestAst::simple(ZoneKindAst::Exile, ZoneOwnerAst::Default);
        if s.contains("face down") {
            d.modifiers.face_down = true;
        }
        if s.contains("at random") {
            d.modifiers.random = true;
        }
        return Some(d);
    }
    if s == "the command zone" || s == "command zone" {
        return Some(ZoneDestAst::simple(
            ZoneKindAst::CommandZone,
            ZoneOwnerAst::Default,
        ));
    }
    if s == "the stack" || s == "stack" {
        return Some(ZoneDestAst::simple(
            ZoneKindAst::Stack,
            ZoneOwnerAst::Default,
        ));
    }

    // Battlefield destination with orthogonal modifiers.
    if let Some(rest) = s
        .strip_prefix("the battlefield")
        .or_else(|| s.strip_prefix("battlefield"))
    {
        let modifiers = parse_battlefield_modifiers(rest.trim())?;
        let owner = modifiers.under_control.unwrap_or(ZoneOwnerAst::Default);
        return Some(ZoneDestAst {
            zone: ZoneKindAst::Battlefield,
            owner,
            library_position: None,
            library_order: None,
            modifiers,
        });
    }

    // Hand destination ("your hand", "its owner's hand", "their owners' hands", etc.).
    if let Some(owner_raw) = s.strip_suffix(" hands").or_else(|| s.strip_suffix(" hand")) {
        let owner = parse_zone_owner(owner_raw)?;
        return Some(ZoneDestAst::simple(ZoneKindAst::Hand, owner));
    }

    // Graveyard destination ("your graveyard", "its owner's graveyard", "their owners' graveyards").
    if let Some(owner_raw) = s
        .strip_suffix(" graveyards")
        .or_else(|| s.strip_suffix(" graveyard"))
    {
        let owner = parse_zone_owner(owner_raw)?;
        return Some(ZoneDestAst::simple(ZoneKindAst::Graveyard, owner));
    }

    // Library destination (top, bottom, choice, ordinal from top, beneath top, shuffled).
    parse_library_dest(s)
}

impl ZoneDestAst {
    fn simple(zone: ZoneKindAst, owner: ZoneOwnerAst) -> Self {
        Self {
            zone,
            owner,
            library_position: None,
            library_order: None,
            modifiers: EntryModifierAst::default(),
        }
    }
}

fn parse_zone_owner(raw: &str) -> Option<ZoneOwnerAst> {
    match raw.trim() {
        "" | "a" | "the" => Some(ZoneOwnerAst::Default),
        "your" => Some(ZoneOwnerAst::You),
        "its owner's" | "his owner's" | "her owner's" | "their owner's" => {
            Some(ZoneOwnerAst::ItsOwner)
        }
        "their owners'" => Some(ZoneOwnerAst::TheirOwners),
        "that player's" | "their" | "that" => Some(ZoneOwnerAst::ThatPlayer),
        "each player's" | "all players'" => Some(ZoneOwnerAst::EachPlayer),
        "an opponent's" | "target opponent's" | "that opponent's" => Some(ZoneOwnerAst::Opponent),
        "that creature's owner" | "that card's owner" | "that permanent's owner" => {
            Some(ZoneOwnerAst::ThatObjectOwner)
        }
        _ => None,
    }
}

fn parse_battlefield_modifiers(mut rest: &str) -> Option<EntryModifierAst> {
    let mut m = EntryModifierAst::default();
    loop {
        rest = rest.trim();
        if rest.is_empty() {
            return Some(m);
        }
        if let Some(r) = rest.strip_prefix("and ") {
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("tapped")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.tapped = true;
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("attacking")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.attacking = true;
            let trimmed_r = r.trim();
            // Capture optional attacking target before any control/face/transformed modifier.
            if let Some(tgt_end) = trimmed_r
                .find(" under ")
                .or_else(|| trimmed_r.find(" tapped"))
                .or_else(|| trimmed_r.find(" face "))
            {
                let tgt = trimmed_r[..tgt_end].trim();
                if !tgt.is_empty() {
                    m.attacking_target = Some(tgt.to_string());
                }
                rest = &trimmed_r[tgt_end..];
            } else if !trimmed_r.is_empty()
                && !trimmed_r.starts_with("under ")
                && !trimmed_r.starts_with("face ")
                && !trimmed_r.starts_with("transformed")
                && !trimmed_r.starts_with("converted")
                && !trimmed_r.starts_with("flipped")
            {
                m.attacking_target = Some(trimmed_r.to_string());
                rest = "";
            } else {
                rest = trimmed_r;
            }
            continue;
        }
        if let Some(r) = rest.strip_prefix("blocking")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.blocking = true;
            rest = r.trim().strip_prefix("that creature").unwrap_or(r.trim());
            continue;
        }
        if let Some(r) = rest.strip_prefix("face up or face down")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.face_up_or_down_choice = true;
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("face down")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.face_down = true;
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("face up")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.face_up = true;
            rest = r;
            continue;
        }
        if let Some(r) = rest
            .strip_prefix("transformed")
            .or_else(|| rest.strip_prefix("converted"))
            && (r.is_empty() || r.starts_with(' '))
        {
            m.transformed = true;
            rest = r;
            continue;
        }
        if let Some(r) = rest.strip_prefix("flipped")
            && (r.is_empty() || r.starts_with(' '))
        {
            m.flipped = true;
            rest = r;
            continue;
        }
        if let Some(after_u) = rest.strip_prefix("under ") {
            if let Some(after_co) = after_u.strip_prefix("the control of ") {
                let (owner_part, rem) = split_control_tail(after_co);
                let owner = match owner_part {
                    "its owner" => ZoneOwnerAst::ItsOwner,
                    "their owners" => ZoneOwnerAst::TheirOwners,
                    "that creature's owner" | "that card's owner" | "that permanent's owner" => {
                        ZoneOwnerAst::ThatObjectOwner
                    }
                    other => parse_zone_owner(other)?,
                };
                m.under_control = Some(owner);
                rest = rem;
                continue;
            }
            if let Some((who_raw, rem)) = after_u.split_once(" control") {
                m.under_control = Some(parse_zone_owner(who_raw)?);
                rest = rem;
                continue;
            }
        }
        return None;
    }
}

fn split_control_tail(s: &str) -> (&str, &str) {
    for kw in [
        " tapped",
        " attacking",
        " face ",
        " transformed",
        " converted",
    ] {
        if let Some(idx) = s.find(kw) {
            return (s[..idx].trim(), s[idx..].trim());
        }
    }
    (s.trim(), "")
}

fn parse_ordinal_word(word: &str) -> Option<u16> {
    match word.trim() {
        "first" | "1st" => Some(1),
        "second" | "2nd" => Some(2),
        "third" | "3rd" => Some(3),
        "fourth" | "4th" => Some(4),
        "fifth" | "5th" => Some(5),
        "sixth" | "6th" => Some(6),
        "seventh" | "7th" => Some(7),
        "eighth" | "8th" => Some(8),
        _ => None,
    }
}

fn parse_library_dest(s: &str) -> Option<ZoneDestAst> {
    // Strip trailing order qualifier (" in a random order" / " in any order").
    let (core, library_order) = if let Some(p) = s.strip_suffix(" in a random order") {
        (p.trim(), Some(LibraryOrderAst::RandomOrder))
    } else if let Some(p) = s.strip_suffix(" in any order") {
        (p.trim(), Some(LibraryOrderAst::AnyOrder))
    } else {
        (s, None)
    };

    // Bare "top" / "the top" / "bottom" / "the bottom"
    match core {
        "top" | "the top" | "top of library" => {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner: ZoneOwnerAst::Default,
                library_position: Some(LibraryPositionAst::Top),
                library_order,
                modifiers: EntryModifierAst::default(),
            });
        }
        "bottom" | "the bottom" | "bottom of library" => {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner: ZoneOwnerAst::Default,
                library_position: Some(LibraryPositionAst::Bottom),
                library_order,
                modifiers: EntryModifierAst::default(),
            });
        }
        _ => {}
    }

    // Choice of top or bottom ("your/their choice of the top or bottom of <owner> library")
    if let Some((_, after_ch)) = core.split_once("choice of the top or bottom of ")
        && let Some(owner_raw) = after_ch
            .strip_suffix(" libraries")
            .or_else(|| after_ch.strip_suffix(" library"))
    {
        return Some(ZoneDestAst {
            zone: ZoneKindAst::Library,
            owner: parse_zone_owner(owner_raw)?,
            library_position: Some(LibraryPositionAst::TopOrBottomChoice),
            library_order,
            modifiers: EntryModifierAst::default(),
        });
    }

    // Top or bottom of <owner> library/libraries
    for (pref, pos) in [
        ("the top of ", LibraryPositionAst::Top),
        ("top of ", LibraryPositionAst::Top),
        ("the bottom of ", LibraryPositionAst::Bottom),
        ("bottom of ", LibraryPositionAst::Bottom),
    ] {
        if let Some(rest) = core.strip_prefix(pref)
            && let Some(owner_raw) = rest
                .strip_suffix(" libraries")
                .or_else(|| rest.strip_suffix(" library"))
        {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner: parse_zone_owner(owner_raw)?,
                library_position: Some(pos),
                library_order,
                modifiers: EntryModifierAst::default(),
            });
        }
    }

    // "<owner> library <ordinal> from the top[ or on the bottom]"
    if let Some((owner_raw, after_lib)) = core.split_once(" library ") {
        let owner = parse_zone_owner(owner_raw)?;
        if let Some(ord_part) = after_lib.strip_suffix(" from the top or on the bottom")
            && let Some(n) = parse_ordinal_word(ord_part)
        {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner,
                library_position: Some(LibraryPositionAst::FromTopOrBottomChoice(n)),
                library_order,
                modifiers: EntryModifierAst::default(),
            });
        }
        if let Some(ord_part) = after_lib.strip_suffix(" from the top")
            && let Some(n) = parse_ordinal_word(ord_part)
        {
            return Some(ZoneDestAst {
                zone: ZoneKindAst::Library,
                owner,
                library_position: Some(LibraryPositionAst::FromTop(n)),
                library_order,
                modifiers: EntryModifierAst::default(),
            });
        }
        if let Some(beneath) = after_lib.strip_prefix("just beneath the top ") {
            let num_part = beneath
                .strip_suffix(" cards")
                .or_else(|| beneath.strip_suffix(" card"))
                .unwrap_or(beneath);
            if let Some(n) = parse_number_word(num_part.trim())
                && n > 0
            {
                return Some(ZoneDestAst {
                    zone: ZoneKindAst::Library,
                    owner,
                    library_position: Some(LibraryPositionAst::BeneathTop(
                        u16::try_from(n).unwrap_or(1),
                    )),
                    library_order,
                    modifiers: EntryModifierAst::default(),
                });
            }
        }
    }

    // "<owner> library" (shuffled / generic)
    if let Some(owner_raw) = core
        .strip_suffix(" libraries")
        .or_else(|| core.strip_suffix(" library"))
    {
        return Some(ZoneDestAst {
            zone: ZoneKindAst::Library,
            owner: parse_zone_owner(owner_raw)?,
            library_position: Some(LibraryPositionAst::Shuffled),
            library_order,
            modifiers: EntryModifierAst::default(),
        });
    }

    None
}

// ---------------------------------------------------------------------------
// 2. RestrictionAst (Timing, Static Rule, and Cast Permission Restrictions)
// ---------------------------------------------------------------------------

/// Typed timing or frequency restriction on activating an ability or casting a spell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimingRestrictionAst {
    SorcerySpeed,
    InstantSpeed,
    OnceEachTurn,
    OncePerGame,
    OnceEachCombat,
    AtMostTimesEachTurn(u16),
    DuringYourTurn,
    DuringOpponentsTurn,
    DuringThatPlayersTurn,
    DuringCombat,
    DuringStep(TurnStepAst),
    BeforeStep(TurnStepAst),
    BeforeCombat,
    BeforeAttackers,
    BeforeBlockers,
    AfterBlockers,
    DuringDeclareAttackers,
    DuringDeclareBlockers,
    DuringCombatDamage,
    WhileOnBattlefield,
    WhileInGraveyard,
    OnlyIfCondition,
    TriggersOnlyIfCondition,
    AnyPlayerCanActivate,
    JoinForcesPayMana,
}

/// Typed static or continuous game-rule / combat restriction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleRestrictionAst {
    CantAttack,
    CantBlock,
    CantBeBlocked,
    CantBeBlockedBy(String),
    MustAttack,
    MustBlock,
    CantBeTargetOf(String),
    CantBeCountered,
    CantCast(String),
    CantPlayLands,
    CantGainLife,
    CantLoseLife,
    CantLoseGame,
    CantWinGame,
    CantDrawMoreThanOne,
    CantTap,
    CantUntap,
    CantActivateAbilities,
    CantHaveCountersPlaced,
    CantTransform,
    CantBeTurnedFaceUp,
    CantBeEnchanted,
    CantBeSacrificed,
    CantBeRegenerated,
    CantPreventDamage,
    Compound(Vec<RuleRestrictionAst>),
}

/// Typed play/cast permission category for `StaticAbilityAst::PlayOrCastPermission`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CastPermissionKindAst {
    CastSelfFromGraveyard,
    CastSelfFromExile,
    PlayFromTopOfLibrary,
    PlayFromGraveyard,
    PlayFromExile,
    CastAsThoughFlash,
    CastWithoutPayingManaCost,
    PlayAdditionalLand,
    CastFromZone(ZoneKindAst),
}

/// Unified typed restriction AST decoded from `AbilityRestriction`, `RuleOrCombatRestriction`,
/// or `PlayOrCastPermission`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestrictionAst {
    Timing(Vec<TimingRestrictionAst>),
    Rule(RuleRestrictionAst),
    CastPermission(CastPermissionKindAst),
}

/// Decode an [`InstructionAst::AbilityRestriction`] string into a typed [`RestrictionAst`].
#[must_use]
pub fn parse_ability_restriction(raw: &str) -> Option<RestrictionAst> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Handle "AnyPlayer:<rest>" composite prefix emitted by the parser.
    let (any_player, body) = match trimmed.strip_prefix("AnyPlayer:") {
        Some(rest) => (true, rest.trim()),
        None => (false, trimmed),
    };

    let mut items = Vec::new();
    if any_player {
        items.push(TimingRestrictionAst::AnyPlayerCanActivate);
    }

    // Handle composite "+" joined restrictions ("DuringYourTurn+OnceEachTurn").
    let parts: Vec<&str> = if body.contains('+') {
        body.split('+')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .collect()
    } else if body.is_empty() {
        Vec::new()
    } else {
        vec![body]
    };

    for part in parts {
        let mut atoms = decode_timing_atom(part)?;
        items.append(&mut atoms);
    }

    if items.is_empty() {
        None
    } else {
        Some(RestrictionAst::Timing(items))
    }
}

fn decode_timing_atom(raw: &str) -> Option<Vec<TimingRestrictionAst>> {
    let low = raw.trim().to_ascii_lowercase();
    let single = match low.as_str() {
        "sorcery" | "as a sorcery" | "activate only as a sorcery" => {
            vec![TimingRestrictionAst::SorcerySpeed]
        }
        "instant" | "as an instant" | "activate only as an instant" => {
            vec![TimingRestrictionAst::InstantSpeed]
        }
        "sorceryonceeachturn" => vec![
            TimingRestrictionAst::SorcerySpeed,
            TimingRestrictionAst::OnceEachTurn,
        ],
        "duringyourturnonceeachturn" => vec![
            TimingRestrictionAst::DuringYourTurn,
            TimingRestrictionAst::OnceEachTurn,
        ],
        "once" | "once_per_game" | "activate only once" => {
            vec![TimingRestrictionAst::OncePerGame]
        }
        "onceeachturn" | "once each turn" | "activate only once each turn" => {
            vec![TimingRestrictionAst::OnceEachTurn]
        }
        "once each combat" | "activate only once each combat" => {
            vec![TimingRestrictionAst::OnceEachCombat]
        }
        "atmost2timeseachturn" | "activate no more than twice each turn" => {
            vec![TimingRestrictionAst::AtMostTimesEachTurn(2)]
        }
        "atmost3timeseachturn" | "activate no more than three times each turn" => {
            vec![TimingRestrictionAst::AtMostTimesEachTurn(3)]
        }
        "duringyourturn" | "during your turn" | "activate only during your turn" => {
            vec![TimingRestrictionAst::DuringYourTurn]
        }
        "duringopponentsturn"
        | "during an opponent's turn"
        | "during opponents' turns"
        | "activate only during an opponent's turn" => {
            vec![TimingRestrictionAst::DuringOpponentsTurn]
        }
        "during their turn" => vec![TimingRestrictionAst::DuringThatPlayersTurn],
        "duringcombat" | "during combat" | "activate only during combat" => {
            vec![TimingRestrictionAst::DuringCombat]
        }
        "duringupkeep"
        | "during your upkeep"
        | "during an opponent's upkeep"
        | "during any player's upkeep"
        | "during any upkeep step" => vec![TimingRestrictionAst::DuringStep(TurnStepAst::Upkeep)],
        "during the end of combat step" => {
            vec![TimingRestrictionAst::DuringStep(TurnStepAst::EndOfCombat)]
        }
        "duringdeclareattackersstep" | "during the declare attackers step" => {
            vec![TimingRestrictionAst::DuringDeclareAttackers]
        }
        "duringdeclareblockersstep" | "during the declare blockers step" => {
            vec![TimingRestrictionAst::DuringDeclareBlockers]
        }
        "during the combat damage step" => vec![TimingRestrictionAst::DuringCombatDamage],
        "before combat" => vec![TimingRestrictionAst::BeforeCombat],
        "before attackers are declared" => vec![TimingRestrictionAst::BeforeAttackers],
        "before blockers are declared" => vec![TimingRestrictionAst::BeforeBlockers],
        "before the end of combat step" => {
            vec![TimingRestrictionAst::BeforeStep(TurnStepAst::EndOfCombat)]
        }
        "before the combat damage step" => vec![TimingRestrictionAst::BeforeBlockers],
        "before the end step" => vec![TimingRestrictionAst::BeforeStep(TurnStepAst::EndStep)],
        "ownturnbeforeattackers" | "during your turn, before attackers are declared" => vec![
            TimingRestrictionAst::DuringYourTurn,
            TimingRestrictionAst::BeforeAttackers,
        ],
        "during an opponent's turn, before attackers are declared" => vec![
            TimingRestrictionAst::DuringOpponentsTurn,
            TimingRestrictionAst::BeforeAttackers,
        ],
        "during your turn before the end step" => vec![
            TimingRestrictionAst::DuringYourTurn,
            TimingRestrictionAst::BeforeStep(TurnStepAst::EndStep),
        ],
        "during their turn before the end step" => vec![
            TimingRestrictionAst::DuringThatPlayersTurn,
            TimingRestrictionAst::BeforeStep(TurnStepAst::EndStep),
        ],
        "during your turn, before the combat damage step" => vec![
            TimingRestrictionAst::DuringYourTurn,
            TimingRestrictionAst::DuringDeclareBlockers,
        ],
        "during combat before blockers are declared" => vec![
            TimingRestrictionAst::DuringCombat,
            TimingRestrictionAst::BeforeBlockers,
        ],
        "during combat after blockers are declared" => vec![
            TimingRestrictionAst::DuringCombat,
            TimingRestrictionAst::AfterBlockers,
        ],
        "while ~ is on the battlefield" => vec![TimingRestrictionAst::WhileOnBattlefield],
        "while ~ is in your graveyard" => vec![TimingRestrictionAst::WhileInGraveyard],
        "condition" => vec![TimingRestrictionAst::OnlyIfCondition],
        "triggers_only_if" => vec![TimingRestrictionAst::TriggersOnlyIfCondition],
        "joinforcespaymana" => vec![TimingRestrictionAst::JoinForcesPayMana],
        _ => {
            // Handle "cast ~ only <timing>" and "activate no more than <N> times each turn"
            if let Some(rest) = low.strip_prefix("cast ~ only ") {
                return decode_timing_atom(rest);
            }
            if let Some(after) = low.strip_prefix("activate no more than ")
                && let Some(cnt_raw) = after.strip_suffix(" times each turn")
                && let Some(n) = parse_number_word(cnt_raw.trim())
                && n > 0
            {
                return Some(vec![TimingRestrictionAst::AtMostTimesEachTurn(
                    u16::try_from(n).unwrap_or(1),
                )]);
            }
            return None;
        }
    };
    Some(single)
}

/// Decode a [`StaticAbilityAst::RuleOrCombatRestriction`] `rule` string into a typed
/// [`RuleRestrictionAst`].
#[must_use]
pub fn parse_rule_restriction(raw: &str) -> Option<RuleRestrictionAst> {
    let trimmed = raw.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(src) = trimmed
        .strip_prefix("CantBeTargetOf:")
        .or_else(|| trimmed.strip_prefix("cant_be_target_of:"))
    {
        return Some(RuleRestrictionAst::CantBeTargetOf(src.trim().to_string()));
    }
    if let Some(flt) = trimmed
        .strip_prefix("cant_cast:")
        .or_else(|| trimmed.strip_prefix("CantCast:"))
    {
        return Some(RuleRestrictionAst::CantCast(flt.trim().to_string()));
    }

    let low = trimmed.to_ascii_lowercase();
    if let Some((a, b)) = low.split_once(" and ")
        && let (Some(r1), Some(r2)) = (parse_rule_restriction(a), parse_rule_restriction(b))
    {
        return Some(RuleRestrictionAst::Compound(vec![r1, r2]));
    }

    match low.as_str() {
        "can't attack" | "cant_attack" => Some(RuleRestrictionAst::CantAttack),
        "can't block" | "cant_block" => Some(RuleRestrictionAst::CantBlock),
        "can't be blocked" | "cant_be_blocked" => Some(RuleRestrictionAst::CantBeBlocked),
        "can't attack or block" | "cant_attack_or_block" => {
            Some(RuleRestrictionAst::Compound(vec![
                RuleRestrictionAst::CantAttack,
                RuleRestrictionAst::CantBlock,
            ]))
        }
        "attacks each combat if able" | "attack each combat if able" | "must_attack" => {
            Some(RuleRestrictionAst::MustAttack)
        }
        "blocks each combat if able" | "block each combat if able" | "must_block" => {
            Some(RuleRestrictionAst::MustBlock)
        }
        "can't be countered" | "cant_be_countered" => Some(RuleRestrictionAst::CantBeCountered),
        "can't play lands" | "cant_play_lands" => Some(RuleRestrictionAst::CantPlayLands),
        "can't gain life" | "players can't gain life" | "opponents can't gain life" => {
            Some(RuleRestrictionAst::CantGainLife)
        }
        "can't lose life" | "your life total can't change" => {
            Some(RuleRestrictionAst::CantLoseLife)
        }
        "you can't lose the game" | "can't lose the game" => Some(RuleRestrictionAst::CantLoseGame),
        "your opponents can't win the game" | "can't win the game" => {
            Some(RuleRestrictionAst::CantWinGame)
        }
        "can't draw more than one card each turn" => Some(RuleRestrictionAst::CantDrawMoreThanOne),
        "can't tap" => Some(RuleRestrictionAst::CantTap),
        "doesn't untap during your untap step"
        | "doesn't untap during its controller's untap step"
        | "don't untap during their controllers' untap steps"
        | "can't untap" => Some(RuleRestrictionAst::CantUntap),
        "can't activate abilities" | "activated abilities can't be activated" => {
            Some(RuleRestrictionAst::CantActivateAbilities)
        }
        "can't have counters placed on it"
        | "can't have counters put on them"
        | "can't have counters put on it" => Some(RuleRestrictionAst::CantHaveCountersPlaced),
        "can't transform" => Some(RuleRestrictionAst::CantTransform),
        "can't be turned face up" => Some(RuleRestrictionAst::CantBeTurnedFaceUp),
        "can't be enchanted" => Some(RuleRestrictionAst::CantBeEnchanted),
        "can't be sacrificed" => Some(RuleRestrictionAst::CantBeSacrificed),
        "can't be regenerated" => Some(RuleRestrictionAst::CantBeRegenerated),
        "damage can't be prevented" => Some(RuleRestrictionAst::CantPreventDamage),
        _ => {
            if let Some(src) = low
                .strip_prefix("can't be the target of ")
                .or_else(|| low.strip_prefix("can't be the targets of "))
            {
                return Some(RuleRestrictionAst::CantBeTargetOf(src.trim().to_string()));
            }
            if let Some(by) = low.strip_prefix("can't be blocked by ") {
                return Some(RuleRestrictionAst::CantBeBlockedBy(by.trim().to_string()));
            }
            if let Some(what) = low.strip_prefix("can't cast ") {
                return Some(RuleRestrictionAst::CantCast(what.trim().to_string()));
            }
            None
        }
    }
}

/// Decode a [`StaticAbilityAst::PlayOrCastPermission`] `permission` string into a typed
/// [`CastPermissionKindAst`].
#[must_use]
pub fn parse_cast_permission(raw: &str) -> Option<CastPermissionKindAst> {
    let low = raw.trim().to_ascii_lowercase();
    match low.as_str() {
        "cast_self_from_graveyard" => Some(CastPermissionKindAst::CastSelfFromGraveyard),
        "cast_self_from_exile" => Some(CastPermissionKindAst::CastSelfFromExile),
        "play_from_top_of_library" | "from the top of your library" => {
            Some(CastPermissionKindAst::PlayFromTopOfLibrary)
        }
        "play_from_graveyard" | "from your graveyard" => {
            Some(CastPermissionKindAst::PlayFromGraveyard)
        }
        "play_from_exile" | "from exile" => Some(CastPermissionKindAst::PlayFromExile),
        "as though they had flash" | "as though it had flash" | "flash" => {
            Some(CastPermissionKindAst::CastAsThoughFlash)
        }
        "without paying their mana costs" | "without paying its mana cost" => {
            Some(CastPermissionKindAst::CastWithoutPayingManaCost)
        }
        "play an additional land" | "additional_land" => {
            Some(CastPermissionKindAst::PlayAdditionalLand)
        }
        _ => {
            if let Some(z_raw) = low
                .strip_prefix("from ")
                .or_else(|| low.strip_prefix("cast_from_"))
                && let Some(dest) = parse_zone_dest(z_raw)
            {
                return Some(CastPermissionKindAst::CastFromZone(dest.zone));
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 3. CombatRuleAst
// ---------------------------------------------------------------------------

/// Typed combat or turn-rule modification decoded from [`InstructionAst::CombatRuleMod::rule`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CombatRuleAst {
    Goaded,
    CantAttack,
    CantBlock,
    CantBeBlocked,
    CantAttackOrBlock,
    MustAttack,
    MustAttackTarget(String),
    MustBlock,
    DoesntUntapNextUntapStep,
    EntersTappedAndAttacking,
    AssignDamageByToughness,
    AssignCombatDamageAsUnblocked,
    AssignNoCombatDamage,
    CanBlockAdditional(u16),
    CanBlockAnyNumber,
    UnblockableExceptBy(String),
    CantCast(String),
    BecomesNight,
    BecomesDay,
    ActivateLoyaltyTwice,
    ExcludeCombatDamageFromPrevention,
    SameIsTrueFor(String),
    Rule(RuleRestrictionAst),
}

/// Decode an [`InstructionAst::CombatRuleMod`] `rule` string into a typed [`CombatRuleAst`].
#[must_use]
pub fn parse_combat_rule(raw: &str) -> Option<CombatRuleAst> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if low.is_empty() {
        return None;
    }

    if let Some(target) = low.strip_prefix("must_attack:") {
        return Some(CombatRuleAst::MustAttackTarget(target.trim().to_string()));
    }
    if let Some(flt) = low.strip_prefix("cant_cast:") {
        return Some(CombatRuleAst::CantCast(flt.trim().to_string()));
    }
    if let Some(tail) = low.strip_prefix("same_is_true_for:") {
        return Some(CombatRuleAst::SameIsTrueFor(tail.trim().to_string()));
    }

    match low.as_str() {
        "goad" | "goaded" => Some(CombatRuleAst::Goaded),
        "can't attack" | "cant_attack" => Some(CombatRuleAst::CantAttack),
        "can't block" | "cant_block" => Some(CombatRuleAst::CantBlock),
        "can't be blocked" | "cant_be_blocked" => Some(CombatRuleAst::CantBeBlocked),
        "can't attack or block" | "cant_attack_or_block" => Some(CombatRuleAst::CantAttackOrBlock),
        "attacks each combat if able"
        | "must attack this turn if able"
        | "attacks this turn if able"
        | "must_attack" => Some(CombatRuleAst::MustAttack),
        "attack_that_opponent_this_turn_if_able" => {
            Some(CombatRuleAst::MustAttackTarget("that opponent".to_string()))
        }
        "blocks this turn if able" | "must block this turn if able" | "must_block" => {
            Some(CombatRuleAst::MustBlock)
        }
        "doesn't untap during its controller's next untap step" | "skip_next_untap" => {
            Some(CombatRuleAst::DoesntUntapNextUntapStep)
        }
        "enters_tapped_and_attacking" | "enters tapped and attacking" => {
            Some(CombatRuleAst::EntersTappedAndAttacking)
        }
        "it becomes night" => Some(CombatRuleAst::BecomesNight),
        "it becomes day" => Some(CombatRuleAst::BecomesDay),
        "activate_loyalty_twice" => Some(CombatRuleAst::ActivateLoyaltyTwice),
        "exclude_combat_damage_from_prevention" => {
            Some(CombatRuleAst::ExcludeCombatDamageFromPrevention)
        }
        "no combat damage" | "assign no combat damage" => Some(CombatRuleAst::AssignNoCombatDamage),
        "can block any number of creatures" => Some(CombatRuleAst::CanBlockAnyNumber),
        _ => {
            if low.contains("assign combat damage equal to") && low.contains("toughness") {
                return Some(CombatRuleAst::AssignDamageByToughness);
            }
            if low.contains("combat damage") && low.contains("as though it weren't blocked") {
                return Some(CombatRuleAst::AssignCombatDamageAsUnblocked);
            }
            if let Some(after) = low.strip_prefix("can block an additional ")
                && let Some(cnt_part) = after
                    .strip_suffix(" creatures")
                    .or_else(|| after.strip_suffix(" creature"))
            {
                let n = if cnt_part.trim().is_empty() {
                    1
                } else {
                    parse_number_word(cnt_part.trim())
                        .and_then(|v| u16::try_from(v).ok())
                        .unwrap_or(1)
                };
                return Some(CombatRuleAst::CanBlockAdditional(n));
            }
            if low == "can block an additional creature" {
                return Some(CombatRuleAst::CanBlockAdditional(1));
            }
            if let Some(exc) = low.strip_prefix("can't be blocked except by ") {
                return Some(CombatRuleAst::UnblockableExceptBy(exc.trim().to_string()));
            }
            parse_rule_restriction(&low).map(CombatRuleAst::Rule)
        }
    }
}

// ---------------------------------------------------------------------------
// 4. ChoiceOptionAst
// ---------------------------------------------------------------------------

/// Typed characteristic or player/object option decoded from [`InstructionAst::ChooseOption`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChoiceOptionAst {
    Color,
    ColorOrColors,
    CreatureType {
        excluded: Option<String>,
    },
    PlaneswalkerType,
    CardType,
    LandType {
        basic_only: bool,
        nonbasic_only: bool,
    },
    CardName {
        nonland_only: bool,
        creature_only: bool,
        excluded: Option<String>,
    },
    Number,
    Letter,
    OddOrEven,
    Player {
        player: PlayerRefAst,
        random: bool,
    },
    AnyTarget,
    Target(String),
    Keywords(Vec<String>),
    CountColors(String),
    FromGroup(String),
    ChoosePlayerToAct(String),
}

/// Decode an [`InstructionAst::ChooseOption`] string into a strongly-typed [`ChoiceOptionAst`].
#[must_use]
pub fn parse_choice_option(raw: &str, vocab: &Vocabulary) -> Option<ChoiceOptionAst> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if low.is_empty() {
        return None;
    }

    if let Some(who) = low.strip_prefix("choose_player:") {
        return Some(ChoiceOptionAst::ChoosePlayerToAct(who.trim().to_string()));
    }
    if let Some(subj) = low.strip_prefix("count_colors:") {
        return Some(ChoiceOptionAst::CountColors(subj.trim().to_string()));
    }

    match low.as_str() {
        "a color" | "color" => Some(ChoiceOptionAst::Color),
        "a color or colors" | "color or colors" => Some(ChoiceOptionAst::ColorOrColors),
        "a creature type" | "creature type" => {
            Some(ChoiceOptionAst::CreatureType { excluded: None })
        }
        "a planeswalker type" | "planeswalker type" => Some(ChoiceOptionAst::PlaneswalkerType),
        "a card type" | "card type" => Some(ChoiceOptionAst::CardType),
        "a basic land type" | "basic land type" => Some(ChoiceOptionAst::LandType {
            basic_only: true,
            nonbasic_only: false,
        }),
        "a nonbasic land type" | "nonbasic land type" => Some(ChoiceOptionAst::LandType {
            basic_only: false,
            nonbasic_only: true,
        }),
        "a land type" | "land type" => Some(ChoiceOptionAst::LandType {
            basic_only: false,
            nonbasic_only: false,
        }),
        "a card name" | "card name" => Some(ChoiceOptionAst::CardName {
            nonland_only: false,
            creature_only: false,
            excluded: None,
        }),
        "a nonland card name" => Some(ChoiceOptionAst::CardName {
            nonland_only: true,
            creature_only: false,
            excluded: None,
        }),
        "a creature card name" => Some(ChoiceOptionAst::CardName {
            nonland_only: false,
            creature_only: true,
            excluded: None,
        }),
        "a number" | "number" => Some(ChoiceOptionAst::Number),
        "a letter" | "letter" => Some(ChoiceOptionAst::Letter),
        "odd or even" | "even or odd" => Some(ChoiceOptionAst::OddOrEven),
        "any target" => Some(ChoiceOptionAst::AnyTarget),
        "an opponent" => Some(ChoiceOptionAst::Player {
            player: PlayerRefAst::AnyOpponent,
            random: false,
        }),
        "an opponent at random" => Some(ChoiceOptionAst::Player {
            player: PlayerRefAst::AnyOpponent,
            random: true,
        }),
        "a player" => Some(ChoiceOptionAst::Player {
            player: PlayerRefAst::AnyPlayer,
            random: false,
        }),
        "a player at random" => Some(ChoiceOptionAst::Player {
            player: PlayerRefAst::AnyPlayer,
            random: true,
        }),
        "another player" => Some(ChoiceOptionAst::Player {
            player: PlayerRefAst::EachOtherPlayer,
            random: false,
        }),
        _ => {
            if let Some(excl) = low.strip_prefix("a creature type other than ") {
                return Some(ChoiceOptionAst::CreatureType {
                    excluded: Some(excl.trim().to_string()),
                });
            }
            if let Some(excl) = low.strip_prefix("a card name other than ") {
                return Some(ChoiceOptionAst::CardName {
                    nonland_only: false,
                    creature_only: false,
                    excluded: Some(excl.trim().to_string()),
                });
            }
            if let Some(kws) = parse_keyword_list(&low, vocab)
                && !kws.is_empty()
            {
                return Some(ChoiceOptionAst::Keywords(kws));
            }
            if low.starts_with("target ") || low.starts_with("any number of target ") {
                return Some(ChoiceOptionAst::Target(low));
            }
            if low.ends_with(" from it") || low.ends_with(" from them") || low.contains(':') {
                return Some(ChoiceOptionAst::FromGroup(low));
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 5. ParsedDurationAst
// ---------------------------------------------------------------------------

/// Typed duration decoded from `GrantCastPermission::duration`,
/// `GrantModifiedCastPermission::duration`, or `ScopedDurationText::duration_text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParsedDurationAst {
    /// Permission applies immediately during resolution of the current spell/ability.
    Immediate,
    /// Maps cleanly onto a core [`DurationAst`].
    Structured(DurationAst),
    /// Active until a specific step (optionally on a player's next turn).
    UntilStep {
        player: Option<PlayerRefAst>,
        step: TurnStepAst,
        next_turn: bool,
    },
    /// Active during a specific player's turn (`"during your turn"`).
    DuringTurn(PlayerRefAst),
    /// Active for as long as the affected card remains exiled.
    ForAsLongAsExiled,
    /// Active for as long as you control the source permanent.
    ForAsLongAsControlled,
    /// Active for the rest of the game.
    RestOfGame,
}

/// Decode a raw duration text string into a strongly-typed [`ParsedDurationAst`].
#[must_use]
pub fn parse_duration_text(raw: &str, vocab: &Vocabulary) -> Option<ParsedDurationAst> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if low.is_empty() {
        return None;
    }

    match low.as_str() {
        "now" | "immediate" | "instant" => Some(ParsedDurationAst::Immediate),
        "until end of turn" | "until_end_of_turn" | "this turn" | "this_turn" => {
            Some(ParsedDurationAst::Structured(DurationAst::UntilEndOfTurn))
        }
        "until end of combat" | "until_end_of_combat" | "this combat" => {
            Some(ParsedDurationAst::Structured(DurationAst::UntilEndOfCombat))
        }
        "until your next turn" | "until_your_next_turn" => Some(ParsedDurationAst::Structured(
            DurationAst::UntilYourNextTurn,
        )),
        "until the end of your next turn"
        | "until_end_of_next_turn"
        | "until the end of your next turn." => Some(ParsedDurationAst::Structured(
            DurationAst::UntilEndOfNextTurn,
        )),
        "until ~ leaves the battlefield" | "until_source_leaves" => Some(
            ParsedDurationAst::Structured(DurationAst::UntilSourceLeaves),
        ),
        "permanent" | "permanently" => Some(ParsedDurationAst::Structured(DurationAst::Permanent)),
        "for the rest of the game" => Some(ParsedDurationAst::RestOfGame),
        "for as long as it remains exiled"
        | "for as long as they remain exiled"
        | "as long as it remains exiled"
        | "for as long as that card remains exiled"
        | "for as long as those cards remain exiled"
        | "while_exiled"
        | "until_next_exile" => Some(ParsedDurationAst::ForAsLongAsExiled),
        "for as long as you control ~" | "as long as you control ~" | "while_control_self" => {
            Some(ParsedDurationAst::ForAsLongAsControlled)
        }
        "during your turn" => Some(ParsedDurationAst::DuringTurn(PlayerRefAst::You)),
        "during that player's turn" | "during their turn" => {
            Some(ParsedDurationAst::DuringTurn(PlayerRefAst::ThatPlayer))
        }
        "until your next end step"
        | "until the beginning of your next end step"
        | "until_your_next_step" => Some(ParsedDurationAst::UntilStep {
            player: Some(PlayerRefAst::You),
            step: TurnStepAst::EndStep,
            next_turn: true,
        }),
        "until the beginning of your next upkeep" | "until your next upkeep" => {
            Some(ParsedDurationAst::UntilStep {
                player: Some(PlayerRefAst::You),
                step: TurnStepAst::Upkeep,
                next_turn: true,
            })
        }
        "until the end of that player's next turn" | "until the end of their next turn" => {
            Some(ParsedDurationAst::Structured(
                DurationAst::UntilEndOfPlayersNextTurn(PlayerRefAst::ThatPlayer),
            ))
        }
        _ => {
            if let Some(cond_raw) = low
                .strip_prefix("for as long as ")
                .or_else(|| low.strip_prefix("as long as "))
                && let Ok(cond) = parse_condition(cond_raw.trim(), vocab)
            {
                return Some(ParsedDurationAst::Structured(DurationAst::ForAsLongAs(
                    Box::new(cond),
                )));
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 6. GrantPayloadAst
// ---------------------------------------------------------------------------

/// Typed granted-ability payload decoded from `GrantAbilityText` or `CreateEmblemFor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GrantPayloadAst {
    /// Pure keyword grant (e.g. `"flying, haste"`).
    Keywords(Vec<String>),
    /// Single activated ability granted directly.
    Activated {
        cost: Vec<CostAtomAst>,
        body: Vec<InstructionAst>,
    },
    /// Single triggered ability granted directly.
    Triggered {
        trigger: TriggerSpecAst,
        body: Vec<InstructionAst>,
    },
    /// Single static ability granted directly.
    Static(Box<StaticAbilityAst>),
    /// Single replacement effect granted directly.
    Replacement(Box<ReplacementEffectAst>),
    /// Multi-ability compiled sub-face (e.g. an emblem or multi-line granted text block).
    CompiledFace(Box<FaceAst>),
}

/// Decode a granted ability or emblem text payload into a typed [`GrantPayloadAst`].
///
/// Reuses `precompiled` when already attached on the AST node, falling back to
/// [`compile_granted_ability`], [`parse_keyword_list`], and [`parse_static_or_replacement`].
#[must_use]
pub fn parse_grant_payload(
    text: &str,
    precompiled: Option<&FaceAst>,
    vocab: &Vocabulary,
) -> Option<GrantPayloadAst> {
    if let Some(face) = precompiled {
        return Some(classify_compiled_face(face));
    }
    let clean = text.trim();
    if clean.is_empty() {
        return None;
    }
    if let Some(kws) = parse_keyword_list(clean, vocab)
        && !kws.is_empty()
    {
        return Some(GrantPayloadAst::Keywords(kws));
    }
    if let Some(face) = compile_granted_ability(clean, vocab) {
        return Some(classify_compiled_face(&face));
    }
    if let Ok(sr) = parse_static_or_replacement(clean, vocab) {
        return Some(match sr {
            StaticOrReplacement::Static(st) => GrantPayloadAst::Static(Box::new(st)),
            StaticOrReplacement::Replacement(rep) => GrantPayloadAst::Replacement(Box::new(rep)),
        });
    }
    None
}

fn classify_compiled_face(face: &FaceAst) -> GrantPayloadAst {
    let total_items = face.abilities.len()
        + face.statics.len()
        + face.replacements.len()
        + usize::from(!face.keywords.is_empty());

    if total_items == 1 {
        if !face.keywords.is_empty() {
            return GrantPayloadAst::Keywords(
                face.keywords.iter().map(|k| k.name.clone()).collect(),
            );
        }
        if let Some(ab) = face.abilities.first() {
            match ab {
                AbilityAst::Activated { cost, body, .. } => {
                    return GrantPayloadAst::Activated {
                        cost: cost.clone(),
                        body: body.clone(),
                    };
                }
                AbilityAst::Triggered { trigger, body, .. } => {
                    return GrantPayloadAst::Triggered {
                        trigger: trigger.clone(),
                        body: body.clone(),
                    };
                }
                _ => {}
            }
        }
        if let Some(st) = face.statics.first() {
            return GrantPayloadAst::Static(Box::new(st.clone()));
        }
        if let Some(rep) = face.replacements.first() {
            return GrantPayloadAst::Replacement(Box::new(rep.clone()));
        }
    }

    GrantPayloadAst::CompiledFace(Box::new(face.clone()))
}

// ---------------------------------------------------------------------------
// 7. ManaSpendRestrictionAst
// ---------------------------------------------------------------------------

/// Polarity of a mana spending restriction (`Spend this mana only ...` vs `can't be spent ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpendPolarity {
    OnlyFor,
    CantBeSpentFor,
}

/// One typed spend target permitted or forbidden by a mana-spend restriction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManaSpendTargetAst {
    CastSpells(ObjectFilterAst),
    ActivateAbilities(Option<ObjectFilterAst>),
    CastCommander,
    CastFromZone(ZoneKindAst),
    PayKeywordOrAbilityCost(String),
}

/// Typed mana spending restriction decoded from [`InstructionAst::ManaSpendRestriction`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaSpendRestrictionAst {
    pub polarity: SpendPolarity,
    pub targets: Vec<ManaSpendTargetAst>,
}

/// Decode an [`InstructionAst::ManaSpendRestriction`] sentence into a strongly-typed
/// [`ManaSpendRestrictionAst`].
#[must_use]
pub fn parse_mana_spend_restriction(
    raw: &str,
    vocab: &Vocabulary,
) -> Option<ManaSpendRestrictionAst> {
    let low = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let (polarity, rest) = if let Some(r) = low
        .strip_prefix("spend this mana only to ")
        .or_else(|| low.strip_prefix("spend that mana only to "))
    {
        (SpendPolarity::OnlyFor, r.trim())
    } else {
        let r = low
            .strip_prefix("this mana can't be spent to ")
            .or_else(|| low.strip_prefix("that mana can't be spent to "))?;
        (SpendPolarity::CantBeSpentFor, r.trim())
    };

    let mut targets = Vec::new();
    // Split top-level " or to " / " or activate " between independent spend clauses.
    let clauses: Vec<&str> = if rest.contains(" or to ") {
        rest.split(" or to ").collect()
    } else if let Some((l, _)) = rest.split_once(" or activate ") {
        vec![l, &rest[l.len() + 4..]] // keeps "activate ..." intact
    } else {
        vec![rest]
    };

    for clause in clauses {
        let target = parse_single_spend_target(clause.trim(), vocab)?;
        targets.push(target);
    }

    if targets.is_empty() {
        None
    } else {
        Some(ManaSpendRestrictionAst { polarity, targets })
    }
}

fn parse_single_spend_target(clause: &str, vocab: &Vocabulary) -> Option<ManaSpendTargetAst> {
    // "cast your commander" / "cast a commander spell"
    if clause == "cast your commander" || clause == "cast a commander" {
        return Some(ManaSpendTargetAst::CastCommander);
    }
    // "cast spells from <zone>"
    if let Some(z_raw) = clause
        .strip_prefix("cast spells from ")
        .or_else(|| clause.strip_prefix("cast a spell from "))
        && let Some(dest) = parse_zone_dest(z_raw)
    {
        return Some(ManaSpendTargetAst::CastFromZone(dest.zone));
    }
    // "cast <filter>"
    if let Some(flt_raw) = clause.strip_prefix("cast ") {
        let flt_clean = flt_raw.trim();
        if let Ok(flt) = parse_object_filter(flt_clean, vocab) {
            return Some(ManaSpendTargetAst::CastSpells(flt));
        }
    }
    // "activate abilities [of <filter>]"
    if clause == "activate abilities" || clause == "activate an ability" {
        return Some(ManaSpendTargetAst::ActivateAbilities(None));
    }
    if let Some(of_raw) = clause
        .strip_prefix("activate abilities of ")
        .or_else(|| clause.strip_prefix("activate an ability of "))
        .or_else(|| clause.strip_prefix("activate mana abilities of "))
        && let Ok(flt) = parse_object_filter(of_raw.trim(), vocab)
    {
        return Some(ManaSpendTargetAst::ActivateAbilities(Some(flt)));
    }
    // "pay <cost/ability>" (e.g. "pay for morph costs", "pay ward costs", "pay cycling costs")
    if let Some(pay_raw) = clause
        .strip_prefix("pay for ")
        .or_else(|| clause.strip_prefix("pay "))
        && !pay_raw.trim().is_empty()
    {
        return Some(ManaSpendTargetAst::PayKeywordOrAbilityCost(
            pay_raw.trim().to_string(),
        ));
    }
    None
}

// ---------------------------------------------------------------------------
// Additive Opt-In Accessors on InstructionAst and StaticAbilityAst
// ---------------------------------------------------------------------------

impl InstructionAst {
    /// Decode this instruction's destination zone string into a strongly-typed [`ZoneDestAst`],
    /// if this instruction carries a zone destination.
    #[must_use]
    pub fn parsed_zone_dest(&self) -> Option<ZoneDestAst> {
        match self {
            Self::MoveZone { to, .. }
            | Self::ReturnZone { to, .. }
            | Self::MoveCounted { to, .. }
            | Self::RevealFromLooked { to, .. } => parse_zone_dest(to),
            Self::SearchLibrary { to_zone, .. } => parse_zone_dest(to_zone),
            Self::ChooseAndKeep { keep_to, .. } => parse_zone_dest(keep_to),
            _ => None,
        }
    }

    /// Decode this instruction's ability restriction string into a strongly-typed [`RestrictionAst`].
    #[must_use]
    pub fn parsed_restriction(&self) -> Option<RestrictionAst> {
        match self {
            Self::AbilityRestriction { restriction, .. } => parse_ability_restriction(restriction),
            Self::TemporaryPlayerRestriction { rule, .. } => {
                parse_rule_restriction(rule).map(RestrictionAst::Rule)
            }
            _ => None,
        }
    }

    /// Decode this instruction's combat-rule string into a strongly-typed [`CombatRuleAst`].
    #[must_use]
    pub fn parsed_combat_rule(&self) -> Option<CombatRuleAst> {
        match self {
            Self::CombatRuleMod { rule, .. } => parse_combat_rule(rule),
            _ => None,
        }
    }

    /// Decode this instruction's choice-option string into a strongly-typed [`ChoiceOptionAst`].
    #[must_use]
    pub fn parsed_choice_option(&self, vocab: &Vocabulary) -> Option<ChoiceOptionAst> {
        match self {
            Self::ChooseOption(opt) => parse_choice_option(opt, vocab),
            _ => None,
        }
    }

    /// Decode this instruction's duration string into a strongly-typed [`ParsedDurationAst`].
    #[must_use]
    pub fn parsed_duration_text(&self, vocab: &Vocabulary) -> Option<ParsedDurationAst> {
        match self {
            Self::GrantCastPermission { duration, .. } => parse_duration_text(duration, vocab),
            Self::GrantModifiedCastPermission(perm) => parse_duration_text(&perm.duration, vocab),
            Self::ScopedDurationText { duration_text, .. } => {
                parse_duration_text(duration_text, vocab)
            }
            _ => None,
        }
    }

    /// Decode this instruction's granted ability or emblem text into a typed [`GrantPayloadAst`].
    #[must_use]
    pub fn parsed_grant_payload(&self, vocab: &Vocabulary) -> Option<GrantPayloadAst> {
        match self {
            Self::GrantAbilityText {
                ability_text,
                compiled,
                ..
            } => parse_grant_payload(ability_text, compiled.as_deref(), vocab),
            Self::CreateEmblemFor { text, compiled, .. } => {
                parse_grant_payload(text, compiled.as_deref(), vocab)
            }
            Self::CreateEmblem(text) => parse_grant_payload(text, None, vocab),
            _ => None,
        }
    }

    /// Decode this instruction's mana-spend restriction string into a typed
    /// [`ManaSpendRestrictionAst`].
    #[must_use]
    pub fn parsed_mana_spend_restriction(
        &self,
        vocab: &Vocabulary,
    ) -> Option<ManaSpendRestrictionAst> {
        match self {
            Self::ManaSpendRestriction(raw) => parse_mana_spend_restriction(raw, vocab),
            _ => None,
        }
    }
}

impl StaticAbilityAst {
    /// Decode this static ability's restriction or cast-permission string into a typed
    /// [`RestrictionAst`].
    #[must_use]
    pub fn parsed_restriction(&self) -> Option<RestrictionAst> {
        match self {
            Self::RuleOrCombatRestriction { rule, .. } => {
                parse_rule_restriction(rule).map(RestrictionAst::Rule)
            }
            Self::PlayOrCastPermission { permission, .. } => {
                parse_cast_permission(permission).map(RestrictionAst::CastPermission)
            }
            _ => None,
        }
    }

    /// Decode this static ability's granted ability text into a typed [`GrantPayloadAst`].
    #[must_use]
    pub fn parsed_grant_payload(&self, vocab: &Vocabulary) -> Option<GrantPayloadAst> {
        match self {
            Self::GrantAbilityText {
                ability_text,
                compiled,
                ..
            } => parse_grant_payload(ability_text, compiled.as_deref(), vocab),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::CardTypeMask;
    use crate::{CardInput, FaceInput, compile_card};

    fn test_vocab() -> Vocabulary {
        let members: Vec<String> = [
            "Dragon",
            "Elf",
            "Goblin",
            "Human",
            "Wizard",
            "Zombie",
            "Spirit",
            "Vampire",
            "Angel",
            "Artifact",
            "Aura",
            "Equipment",
            "Shrine",
            "Pirate",
            "Rogue",
            "Warrior",
            "Cleric",
            "Island",
            "Swamp",
            "Mountain",
            "Forest",
            "Plains",
            "Treasure",
            "Food",
            "Clue",
            "Fractal",
        ]
        .into_iter()
        .map(ToString::to_string)
        .collect();
        Vocabulary::from_creature_members(&members)
    }

    #[test]
    fn zone_dest_parses_printed_and_custom_destinations() {
        let bf = parse_zone_dest(
            "the battlefield tapped and attacking defending player under your control",
        )
        .expect("battlefield tapped attacking");
        assert_eq!(bf.zone, ZoneKindAst::Battlefield);
        assert_eq!(bf.owner, ZoneOwnerAst::You);
        assert!(bf.modifiers.tapped);
        assert!(bf.modifiers.attacking);
        assert_eq!(
            bf.modifiers.attacking_target.as_deref(),
            Some("defending player")
        );
        assert_eq!(bf.modifiers.under_control, Some(ZoneOwnerAst::You));

        let lib = parse_zone_dest("the bottom of its owner's library in a random order")
            .expect("bottom random");
        assert_eq!(lib.zone, ZoneKindAst::Library);
        assert_eq!(lib.owner, ZoneOwnerAst::ItsOwner);
        assert_eq!(lib.library_position, Some(LibraryPositionAst::Bottom));
        assert_eq!(lib.library_order, Some(LibraryOrderAst::RandomOrder));

        let ord = parse_zone_dest("your library third from the top").expect("third from top");
        assert_eq!(ord.library_position, Some(LibraryPositionAst::FromTop(3)));

        let choice = parse_zone_dest("the battlefield or into your hand").expect("choice dest");
        assert!(matches!(choice.zone, ZoneKindAst::Choice(ref v) if v.len() == 2));

        let stay = parse_zone_dest("stay_in:your graveyard").expect("stay_in");
        assert_eq!(
            stay.zone,
            ZoneKindAst::StayIn(Box::new(ZoneKindAst::Graveyard))
        );
        assert_eq!(stay.owner, ZoneOwnerAst::You);

        let reinc = parse_zone_dest("the battlefield under the control of that creature's owner")
            .expect("reincarnation dest");
        assert_eq!(reinc.owner, ZoneOwnerAst::ThatObjectOwner);
    }

    #[test]
    fn restrictions_decode_timing_rules_and_cast_permissions() {
        let combo = parse_ability_restriction("AnyPlayer:DuringYourTurn+OnceEachTurn")
            .expect("combo timing");
        assert_eq!(
            combo,
            RestrictionAst::Timing(vec![
                TimingRestrictionAst::AnyPlayerCanActivate,
                TimingRestrictionAst::DuringYourTurn,
                TimingRestrictionAst::OnceEachTurn,
            ])
        );

        let custom_cap = parse_ability_restriction("activate no more than four times each turn")
            .expect("custom cap");
        assert_eq!(
            custom_cap,
            RestrictionAst::Timing(vec![TimingRestrictionAst::AtMostTimesEachTurn(4)])
        );

        let rule = parse_rule_restriction("can't attack and can't block").expect("compound rule");
        assert_eq!(
            rule,
            RuleRestrictionAst::Compound(vec![
                RuleRestrictionAst::CantAttack,
                RuleRestrictionAst::CantBlock,
            ])
        );

        let target_rule =
            parse_rule_restriction("CantBeTargetOf:spells or abilities your opponents control")
                .expect("cant be target");
        assert_eq!(
            target_rule,
            RuleRestrictionAst::CantBeTargetOf(
                "spells or abilities your opponents control".to_string()
            )
        );

        let perm = parse_cast_permission("cast_self_from_graveyard").expect("cast perm");
        assert_eq!(perm, CastPermissionKindAst::CastSelfFromGraveyard);
    }

    #[test]
    fn combat_rule_and_choice_option_and_duration_decode_cleanly() {
        let vocab = test_vocab();

        assert_eq!(
            parse_combat_rule("must_attack:target opponent"),
            Some(CombatRuleAst::MustAttackTarget(
                "target opponent".to_string()
            ))
        );
        assert_eq!(
            parse_combat_rule("can block an additional two creatures"),
            Some(CombatRuleAst::CanBlockAdditional(2))
        );
        assert_eq!(
            parse_combat_rule("can't be blocked except by creatures with flying"),
            Some(CombatRuleAst::UnblockableExceptBy(
                "creatures with flying".to_string()
            ))
        );

        assert_eq!(
            parse_choice_option("a creature type other than Wall", &vocab),
            Some(ChoiceOptionAst::CreatureType {
                excluded: Some("wall".to_string())
            })
        );
        assert_eq!(
            parse_choice_option("flying, first strike, or vigilance", &vocab),
            Some(ChoiceOptionAst::Keywords(vec![
                "flying".to_string(),
                "first strike".to_string(),
                "vigilance".to_string(),
            ]))
        );

        assert_eq!(
            parse_duration_text("now", &vocab),
            Some(ParsedDurationAst::Immediate)
        );
        assert_eq!(
            parse_duration_text("for as long as it remains exiled", &vocab),
            Some(ParsedDurationAst::ForAsLongAsExiled)
        );
        assert!(matches!(
            parse_duration_text("for as long as you control an artifact", &vocab),
            Some(ParsedDurationAst::Structured(DurationAst::ForAsLongAs(_)))
        ));
    }

    #[test]
    fn mana_spend_restriction_and_grant_payload_decode_cleanly() {
        let vocab = test_vocab();

        let msr = parse_mana_spend_restriction(
            "Spend this mana only to cast Dragon spells or activate abilities of Dragons.",
            &vocab,
        )
        .expect("dragon mana spend");
        assert_eq!(msr.polarity, SpendPolarity::OnlyFor);
        assert_eq!(msr.targets.len(), 2);
        assert!(matches!(
            &msr.targets[0],
            ManaSpendTargetAst::CastSpells(f) if f.subtypes.iter().any(|s| s == "Dragon")
        ));
        assert!(matches!(
            &msr.targets[1],
            ManaSpendTargetAst::ActivateAbilities(Some(f)) if f.subtypes.iter().any(|s| s == "Dragon")
        ));

        let neg_msr = parse_mana_spend_restriction(
            "This mana can't be spent to cast noncreature spells.",
            &vocab,
        )
        .expect("negated mana spend");
        assert_eq!(neg_msr.polarity, SpendPolarity::CantBeSpentFor);
        assert!(matches!(
            &neg_msr.targets[0],
            ManaSpendTargetAst::CastSpells(f)
                if (f.forbidden_types.0 & CardTypeMask::CREATURE.0) != 0
        ));

        let kw_payload = parse_grant_payload("flying and lifelink", None, &vocab).expect("kws");
        assert_eq!(
            kw_payload,
            GrantPayloadAst::Keywords(vec!["flying".to_string(), "lifelink".to_string()])
        );

        let trig_payload =
            parse_grant_payload("When ~ dies, draw a card.", None, &vocab).expect("trig");
        assert!(matches!(trig_payload, GrantPayloadAst::Triggered { .. }));
    }

    #[test]
    fn ast_accessors_work_end_to_end_on_compiled_custom_card() {
        let vocab = test_vocab();
        let card = compile_card(
            &CardInput {
                oracle_id: "custom-typed-ir-probe",
                name: "Chronicle Archivist",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Chronicle Archivist",
                    type_line: "Creature — Human Wizard",
                    oracle_text: "{T}: Add {U}{R}. Spend this mana only to cast Wizard spells or activate abilities of Wizards.\n{2}{U}, {T}: Return target instant or sorcery card from your graveyard to your hand. Activate only as a sorcery and only once each turn.",
                }],
            },
            &vocab,
        );
        assert!(
            card.strictly_compiled,
            "failures: {:?}",
            card.faces[0].failures
        );

        let AbilityAst::Activated { body: b0, .. } = &card.faces[0].abilities[0] else {
            panic!("expected activated ability 0");
        };
        let msr = b0
            .iter()
            .find_map(|ins| ins.parsed_mana_spend_restriction(&vocab))
            .expect("decoded mana spend restriction");
        assert_eq!(msr.polarity, SpendPolarity::OnlyFor);
        assert_eq!(msr.targets.len(), 2);

        let AbilityAst::Activated { body: b1, .. } = &card.faces[0].abilities[1] else {
            panic!("expected activated ability 1");
        };
        let dest = b1
            .iter()
            .find_map(InstructionAst::parsed_zone_dest)
            .expect("decoded zone dest");
        assert_eq!(dest.zone, ZoneKindAst::Hand);
        assert_eq!(dest.owner, ZoneOwnerAst::You);

        let restr = b1
            .iter()
            .find_map(InstructionAst::parsed_restriction)
            .expect("decoded restriction");
        assert_eq!(
            restr,
            RestrictionAst::Timing(vec![
                TimingRestrictionAst::SorcerySpeed,
                TimingRestrictionAst::OnceEachTurn,
            ])
        );
    }
}
