//! Mana specification (`ManaSpecAst`) and token creation (`TokenSpecAst`) parsers.

use super::object::parse_object_filter;
use super::subject::{parse_keyword_list, parse_player_ref, parse_subject_or_target};
use super::value::{parse_for_each_value_expr, parse_value_expr};
use crate::ast::{
    CardTypeMask, ColorMask, ControllerRelation, ManaSpecAst, PlayerRefAst, Rounding,
    SubjectRefAst, SupertypeMask, TokenSpecAst, ValueExprAst,
};
use crate::combinators::{
    split_once_any, strip_prefix_any, strip_suffix_any, trim_punct, val_div, vconst,
};
use crate::preprocess::{is_mana_sequence, parse_base_pt, split_top_level};
use crate::vocab::{Vocabulary, lookup_card_type, lookup_color, lookup_supertype};

/// Strictly parse a mana specification (`{G}`, `{W} or {U}`, `one mana of any color`, etc.).
pub fn parse_mana_spec(phrase: &str, vocab: &Vocabulary) -> Result<ManaSpecAst, String> {
    let raw = trim_punct(phrase);
    if is_mana_sequence(raw) {
        return Ok(ManaSpecAst::Fixed(raw.to_string()));
    }
    // Multi-way pure mana sequence choice FIRST: "{W} or {U}", "{R}, {G}, or {W}", "{W} or {U} or {B}"
    let normalized_choice = raw.replace(", or ", " or ").replace(", ", " or ");
    let parts: Vec<&str> = normalized_choice
        .split(" or ")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 && parts.iter().all(|p| is_mana_sequence(p)) {
        return Ok(ManaSpecAst::Choice(
            parts.into_iter().map(ToString::to_string).collect(),
        ));
    }
    // Mixed non-symbol mana choices (e.g. "{W} or one mana of the chosen color", "X {G} or X {W}")
    // are emitted as typed `InstructionAst::AddQualifiedMana { choices: Vec<ManaSpecAst> }` in clauses.rs.
    // "<value_expr> {M}" e.g. "six {R}", "three {G}", "twice that much {G}", "that much {R}", "X {B}"
    if let Some((amt_raw, sym)) = raw.rsplit_once(' ')
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_value_expr(amt_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }
    if let Some(after) = raw.to_ascii_lowercase().strip_prefix("an amount of ")
        && let Some((sym_part, val_raw)) = after.split_once(" equal to ")
        && let Ok(amt) = parse_value_expr(val_raw.trim(), vocab)
    {
        let sym_orig = raw["an amount of ".len()..][..sym_part.len()].trim();
        if is_mana_sequence(sym_orig) {
            return Ok(ManaSpecAst::ScaledSymbol {
                symbol: sym_orig.to_string(),
                amount: Box::new(amt),
            });
        }
    }

    let low = raw.to_ascii_lowercase();
    // Handled as typed AddQualifiedMana choices in clauses.rs (avoid raw prose in ManaSpecAst::Choice).
    if low == "one mana of the chosen color" || low == "one mana of that color" {
        return Ok(ManaSpecAst::ChosenColor);
    }
    if low == "one mana of any color in your commander's color identity" {
        return Ok(ManaSpecAst::CommanderColorIdentity);
    }
    if low == "one mana of any type that a land you control could produce"
        || low == "one mana of any color that a land you control could produce"
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::You,
        });
    }
    if low == "one mana of any type that a land an opponent controls could produce"
        || low == "one mana of any color that a land an opponent controls could produce"
    {
        return Ok(ManaSpecAst::AnyLandCouldProduce {
            controller: ControllerRelation::Opponent,
        });
    }
    // Cluster 11: Generalized lossless `AnyObjectCouldProduce` (Gond Gate, etc.)
    if let Some((any_type, rest)) = low
        .strip_prefix("one mana of any type that ")
        .map(|r| (true, r))
        .or_else(|| {
            low.strip_prefix("one mana of any color that ")
                .map(|r| (false, r))
        })
        && let Some(flt_raw) = rest.strip_suffix(" could produce")
        && let Ok(flt) = parse_object_filter(flt_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::AnyObjectCouldProduce {
            filter: Box::new(flt),
            any_type,
        });
    }
    // Cluster 11: Lossless `AnyColorAmong` (Mox Amber, Pit of Offerings, etc.)
    if let Some(subj_raw) = low.strip_prefix("one mana of any color among ")
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::AnyColorAmong {
            source: Box::new(subj),
        });
    }
    if let Some(subj_raw) = low
        .strip_prefix("one mana of any of ")
        .and_then(|r| strip_suffix_any(r, &["'s colors", "' colors"]))
        && let Ok(subj) = parse_subject_or_target(subj_raw.trim(), vocab)
    {
        return Ok(ManaSpecAst::AnyColorAmong {
            source: Box::new(subj),
        });
    }
    if let Some((sym, fe_raw)) = raw.split_once(" for each ")
        && is_mana_sequence(sym.trim())
        && fe_raw
            .to_ascii_lowercase()
            .contains("counter removed this way")
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(ValueExprAst::PriorResult),
        });
    }
    if let Some((amt_raw, syms_raw)) = raw.split_once(" mana in any combination of ")
        && let Ok(amt) = parse_value_expr(amt_raw, vocab)
    {
        let cleaned = syms_raw
            .replace(" and/or ", " ")
            .replace(" or ", " ")
            .replace(", ", " ");
        let syms: Vec<String> = cleaned
            .split_whitespace()
            .filter(|t| is_mana_sequence(t))
            .map(ToString::to_string)
            .collect();
        if !syms.is_empty() {
            return Ok(ManaSpecAst::CombinationOf {
                amount: Box::new(amt),
                symbols: syms,
            });
        }
    }
    if let Some(amt_str) = low.strip_suffix(" mana of different colors")
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::DifferentColors {
            amount: Box::new(amt),
        });
    }
    // "one mana of any [one] color for each <fe>" (Coalition Relic, etc.)
    if let Some((left, fe_raw)) = low.split_once(" for each ")
        && (left.ends_with(" mana of any one color")
            || left.ends_with(" mana of any color")
            || left.ends_with(" mana in any combination of colors"))
        && let Ok(scale) = parse_for_each_value_expr(fe_raw, vocab)
    {
        let single_color = left.ends_with(" mana of any one color");
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(scale),
            single_color,
        });
    }
    if let Some(amt_str) = strip_suffix_any(&low, &[" mana of any one color", " mana of any color"])
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amt),
            single_color: low.contains("any one color") || low.starts_with("one "),
        });
    }
    if let Some(amt_str) = low.strip_suffix(" mana in any combination of colors")
        && let Ok(amt) = parse_value_expr(amt_str, vocab)
    {
        return Ok(ManaSpecAst::AnyColor {
            amount: Box::new(amt),
            single_color: false,
        });
    }
    if let Some(after) = raw.strip_prefix("an amount of ")
        && let Some((sym, val_raw)) = after.split_once(" equal to ")
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_value_expr(val_raw, vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }
    if low == "an amount of mana of that color equal to your devotion to that color" {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: "chosen_color".to_string(),
            amount: Box::new(ValueExprAst::Devotion {
                colors: "chosen_color".to_string(),
            }),
        });
    }
    // "that much {M}" (Rasputin, the Oneiromancer)
    if let Some(sym) = raw.strip_prefix("that much ")
        && is_mana_sequence(sym.trim())
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(ValueExprAst::PriorResult),
        });
    }
    // "{B} or {G} for each <fe>" (Culling Ritual, etc.)
    if let Some((left_mana, fe_raw)) = raw.split_once(" for each ")
        && let Ok(amt) = parse_for_each_value_expr(fe_raw, vocab)
    {
        let normalized_choice = left_mana.replace(", or ", " or ").replace(", ", " or ");
        let syms: Vec<String> = normalized_choice
            .split(" or ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(ToString::to_string)
            .collect();
        if syms.len() >= 2 && syms.iter().all(|p| is_mana_sequence(p)) {
            return Ok(ManaSpecAst::CombinationOf {
                amount: Box::new(amt),
                symbols: syms,
            });
        }
    }
    if let Some((sym, fe_raw)) = raw.split_once(" for each ")
        && is_mana_sequence(sym.trim())
        && let Ok(amt) = parse_for_each_value_expr(fe_raw, vocab)
    {
        return Ok(ManaSpecAst::ScaledSymbol {
            symbol: sym.trim().to_string(),
            amount: Box::new(amt),
        });
    }

    Err(format!("unparsed_mana:{raw}"))
}

const PREDEFINED_TOKENS: &[&str] = &[
    "treasure",
    "food",
    "clue",
    "blood",
    "map",
    "powerstone",
    "junk",
    "gold",
    "shard",
    "incubator",
    "lander",
    "mutagen",
    "walker",
    "role",
    "heartwood",
    "vibranium",
];

/// Strictly parse the object of `create ...` into a typed `TokenSpecAst`.
/// Rich token creation parse result preserving blocking status, dynamic P/T expressions, and chosen color/type qualifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichTokenCreationAst {
    pub token: TokenSpecAst,
    pub blocking: bool,
    pub dynamic_power: Option<ValueExprAst>,
    pub dynamic_toughness: Option<ValueExprAst>,
    pub dynamic_pt_text: Option<String>,
    pub chosen_color: bool,
    pub chosen_type: bool,
    pub attacking_target: Option<SubjectRefAst>,
}

impl RichTokenCreationAst {
    /// True when no extra qualifiers beyond plain `TokenSpecAst` are attached.
    #[must_use]
    pub const fn is_plain(&self) -> bool {
        !self.blocking
            && self.dynamic_power.is_none()
            && self.dynamic_toughness.is_none()
            && self.dynamic_pt_text.is_none()
            && !self.chosen_color
            && !self.chosen_type
            && self.attacking_target.is_none()
    }
}

pub fn parse_token_creation(phrase: &str, vocab: &Vocabulary) -> Result<TokenSpecAst, String> {
    parse_token_creation_rich(phrase, vocab).map(|r| r.token)
}

pub fn parse_token_creation_rich(
    phrase: &str,
    vocab: &Vocabulary,
) -> Result<RichTokenCreationAst, String> {
    let raw = trim_punct(phrase);
    let mut low = raw.to_ascii_lowercase();
    let mut tapped = false;
    let mut attacking = false;
    let mut blocking = false;
    let mut attacking_target: Option<SubjectRefAst> = None;
    let parse_atk_target = |tail: &str| -> Option<SubjectRefAst> {
        let t = tail.trim();
        if t.is_empty() {
            return None;
        }
        if let Some(p) = parse_player_ref(t) {
            return Some(SubjectRefAst::Player(p));
        }
        parse_subject_or_target(t, vocab).ok()
    };

    if let Some((before, after)) = split_once_any(
        &low,
        &[
            " that's tapped and attacking",
            " that are tapped and attacking",
        ],
    ) {
        tapped = true;
        attacking = true;
        attacking_target = parse_atk_target(after);
        low = before.to_string();
    } else if low.contains(" tapped and attacking") {
        tapped = true;
        attacking = true;
        for (suffix, tgt) in [
            (
                " tapped and attacking that player",
                Some(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
            ),
            (
                " tapped and attacking defending player",
                Some(SubjectRefAst::Player(PlayerRefAst::DefendingPlayer)),
            ),
            (
                " tapped and attacking you",
                Some(SubjectRefAst::Player(PlayerRefAst::You)),
            ),
            (
                " tapped and attacking that opponent",
                Some(SubjectRefAst::Player(PlayerRefAst::ThatPlayer)),
            ),
            (
                " tapped and attacking an opponent",
                Some(SubjectRefAst::Player(PlayerRefAst::AnyOpponent)),
            ),
            (" tapped and attacking", None),
        ] {
            if let Some(stripped) = low.strip_suffix(suffix) {
                attacking_target = tgt;
                low = stripped.to_string();
                break;
            }
        }
        if low.contains(" tapped and attacking") {
            low = low.replace(" tapped and attacking", "");
        }
    }
    if let Some((before, after)) =
        split_once_any(&low, &[" that's attacking", " that are attacking"])
    {
        attacking = true;
        if attacking_target.is_none() {
            attacking_target = parse_atk_target(after);
        }
        low = before.to_string();
    } else {
        for (pat, tgt) in [
            (
                " attacking that opponent",
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
            ),
            (
                " attacking that player",
                SubjectRefAst::Player(PlayerRefAst::ThatPlayer),
            ),
            (
                " attacking defending player",
                SubjectRefAst::Player(PlayerRefAst::DefendingPlayer),
            ),
            (
                " attacking an opponent",
                SubjectRefAst::Player(PlayerRefAst::AnyOpponent),
            ),
            (" attacking you", SubjectRefAst::Player(PlayerRefAst::You)),
        ] {
            if let Some((before, _)) = low.split_once(pat) {
                attacking = true;
                if attacking_target.is_none() {
                    attacking_target = Some(tgt);
                }
                low = before.to_string();
                break;
            }
        }
    }
    let wrap_plain = |token: TokenSpecAst, blk: bool| RichTokenCreationAst {
        token,
        blocking: blk,
        dynamic_power: None,
        dynamic_toughness: None,
        dynamic_pt_text: None,
        chosen_color: false,
        chosen_type: false,
        attacking_target: attacking_target.clone(),
    };
    if let Some((before, _)) = split_once_any(&low, &[" that's blocking", " that are blocking"]) {
        blocking = true;
        low = before.to_string();
    }
    if low.contains(" that's tapped") {
        tapped = true;
        low = low.replace(" that's tapped", "");
    } else if low.contains(" tapped") {
        tapped = true;
        low = low.replace(" tapped", "");
    }
    let low = low.trim();

    // "N/X of those tokens" / "another one of those tokens" (Wurmquake, etc.)
    if low == "another one of those tokens" {
        return Ok(wrap_plain(
            TokenSpecAst::Predefined {
                name: "those_tokens".to_string(),
                count: vconst(1),
                tapped,
            },
            blocking,
        ));
    }
    if let Some(cnt_part) = low.strip_suffix(" of those tokens")
        && let Ok(cnt) = parse_value_expr(cnt_part, vocab)
    {
        return Ok(wrap_plain(
            TokenSpecAst::Predefined {
                name: "those_tokens".to_string(),
                count: Box::new(cnt),
                tapped,
            },
            blocking,
        ));
    }

    // "half X <desc> tokens, rounded up/down" (The Goose Mother, etc.)
    if let Some(after_half) = low.strip_prefix("half ")
        && let Some((tok_side, round_raw)) = after_half.split_once(", rounded ")
        && let Some((expr_tok, rest_tok)) = tok_side.split_once(' ')
        && let Ok(inner_val) = parse_value_expr(expr_tok, vocab)
        && let Ok(mut tok) = parse_token_creation(&format!("a {}", rest_tok.trim()), vocab)
    {
        let div_val = ValueExprAst::DivConst {
            expr: Box::new(inner_val),
            denom: 2,
            rounding: if round_raw.trim() == "up" {
                crate::ast::Rounding::Up
            } else {
                crate::ast::Rounding::Down
            },
        };
        match &mut tok {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => **count = div_val,
            TokenSpecAst::Role { .. } => {}
        }
        return Ok(wrap_plain(tok, blocking));
    }

    // Copy token: "a token that's a copy of <subject> [tapped and attacking ...][, except <exception>]"
    if let Some((cnt_part, rest)) = split_once_any(
        &low,
        &[" token that's a copy of ", " tokens that are copies of "],
    ) {
        let (src_raw_full, exception) = match rest.split_once(", except ") {
            Some((s, exc)) => (s, Some(exc.trim().to_string())),
            None => (rest, None),
        };
        let src_raw = split_once_any(
            &src_raw_full,
            &[" tapped and attacking", " and that's blocking"],
        )
        .map_or(src_raw_full, |(s, _)| s)
        .trim();
        if let (Ok(cnt), Ok(subj)) = (
            parse_value_expr(cnt_part, vocab),
            parse_subject_or_target(src_raw, vocab),
        ) {
            return Ok(wrap_plain(
                TokenSpecAst::Copy {
                    count: Box::new(cnt),
                    source: Box::new(subj),
                    tapped: tapped || src_raw_full.contains("tapped"),
                    attacking: attacking || src_raw_full.contains("attacking"),
                    exception,
                },
                blocking || src_raw_full.contains("blocking"),
            ));
        }
    }
    // Dynamic P/T creature token: "<cnt> <colors> <subtypes> creature token with [base] power and toughness each equal to <val>"
    // or "with power equal to <v1> and toughness equal to <v2>" (Skullspore Nexus, Soul Separator)
    if let Some((left_desc, rhs, is_each_equal)) = low
        .split_once(" creature token with base power and toughness each equal to ")
        .map(|(l, r)| (l, r, true))
        .or_else(|| {
            low.split_once(" creature token with power and toughness each equal to ")
                .map(|(l, r)| (l, r, true))
        })
        .or_else(|| {
            low.split_once(" creature token with power equal to ")
                .map(|(l, r)| (l, r, false))
        })
        && let Some((cnt_tok, rest_desc)) = left_desc.split_once(' ')
        && let Ok(cnt) = parse_value_expr(cnt_tok, vocab)
    {
        let mut colors = ColorMask::EMPTY;
        let mut supertypes = SupertypeMask::EMPTY;
        let mut card_types = CardTypeMask::CREATURE;
        let mut subtypes = Vec::new();
        for w in rest_desc.split_whitespace() {
            let tok = w.trim_matches(',');
            if tok == "and" || tok == "colorless" {
                continue;
            }
            if let Some(c) = lookup_color(tok) {
                colors = colors.union(c);
            } else if let Some(st) = lookup_supertype(tok) {
                supertypes = supertypes.union(st);
            } else if let Some(ct) = lookup_card_type(tok) {
                card_types = card_types.union(ct);
            } else if let Some(sub) = vocab.lookup_subtype(tok) {
                subtypes.push(sub.to_string());
            } else {
                subtypes.push(tok.to_string());
            }
        }
        let (rhs_clean, extra_kws) =
            if let Some((expr_part, kw_part)) = split_once_any(&rhs, &[" that has ", " with "]) {
                let kws = parse_keyword_list(kw_part.trim(), vocab)
                    .unwrap_or_else(|| vec![kw_part.trim().to_string()]);
                (expr_part.trim(), kws)
            } else {
                (rhs.trim(), Vec::new())
            };
        let (dyn_p, dyn_t, dyn_text) = if is_each_equal {
            match parse_value_expr(rhs_clean, vocab) {
                Ok(v) => (Some(v.clone()), Some(v), None),
                Err(_) => (None, None, Some(rhs_clean.to_string())),
            }
        } else if let Some((p_raw, t_raw)) = rhs_clean.split_once(" and toughness equal to ") {
            let pv = parse_value_expr(p_raw.trim(), vocab).ok();
            let tv = parse_value_expr(t_raw.trim(), vocab).ok();
            let txt = if pv.is_none() || tv.is_none() {
                Some(rhs_clean.to_string())
            } else {
                None
            };
            (pv, tv, txt)
        } else {
            match parse_value_expr(rhs_clean, vocab) {
                Ok(v) => (Some(v), None, None),
                Err(_) => (None, None, Some(rhs_clean.to_string())),
            }
        };
        return Ok(RichTokenCreationAst {
            token: TokenSpecAst::Creature {
                count: Box::new(cnt),
                power: "X".to_string(),
                toughness: "X".to_string(),
                colors,
                supertypes,
                card_types,
                subtypes,
                keywords: extra_kws,
                tapped,
                attacking,
                named: None,
            },
            blocking,
            dynamic_power: dyn_p,
            dynamic_toughness: dyn_t,
            dynamic_pt_text: dyn_text,
            chosen_color: false,
            chosen_type: false,
            attacking_target,
        });
    }

    // Peel optional trailing "that's all colors" or "that's <colors>" (Godsire)
    let (low, all_colors, extra_colors) = if let Some(stripped) =
        low.strip_suffix(" that's all colors")
    {
        (stripped.trim(), true, ColorMask::EMPTY)
    } else if let Some((before, color_list)) = split_once_any(&low, &[" that's ", " that are "]) {
        let mut parsed_c = ColorMask::EMPTY;
        let mut all_valid = true;
        for w in color_list.split_whitespace() {
            let tok = w.trim_matches(',');
            if tok == "and" {
                continue;
            }
            if let Some(c) = lookup_color(tok) {
                parsed_c = parsed_c.union(c);
            } else {
                all_valid = false;
                break;
            }
        }
        if all_valid && !parsed_c.is_empty() {
            (before.trim(), false, parsed_c)
        } else {
            (low, false, ColorMask::EMPTY)
        }
    } else {
        (low, false, ColorMask::EMPTY)
    };

    // Peel optional trailing "for each ..." ONLY when not inside a quoted token ability
    let (body, for_each_expr) = if !low.contains('"')
        && let Some((b, fe)) = low.split_once(" for each ")
    {
        let fe_val = parse_for_each_value_expr(fe, vocab)?;
        (b.trim(), Some(fe_val))
    } else {
        (low, None)
    };

    // Wilds of Eldraine Role tokens: "a <RoleName> Role token attached to <target>"
    if let Some((role_part, att_raw)) = low.split_once(" role token attached to ")
        && let Ok(attached_to) = parse_subject_or_target(att_raw, vocab)
    {
        let role_name = strip_prefix_any(&role_part, &["a ", "an "])
            .unwrap_or(role_part)
            .trim()
            .to_string();
        if !role_name.is_empty() {
            return Ok(wrap_plain(
                TokenSpecAst::Role {
                    role_name,
                    attached_to: Box::new(attached_to),
                },
                blocking,
            ));
        }
    }

    // Predefined token: "a Treasure token", "two Food tokens"
    for &ptok in PREDEFINED_TOKENS {
        if let Some(cnt_str) = body
            .strip_suffix(&format!(" {ptok} token"))
            .or_else(|| body.strip_suffix(&format!(" {ptok} tokens")))
        {
            let base_cnt = parse_value_expr(cnt_str, vocab)?;
            let count = match for_each_expr {
                Some(fe) if base_cnt != ValueExprAst::Const(1) => {
                    ValueExprAst::Mul(Box::new(base_cnt), Box::new(fe))
                }
                Some(fe) => fe,
                None => base_cnt,
            };
            let mut chars = ptok.chars();
            let cap = match chars.next() {
                Some(c) => format!("{}{}", c.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            };
            return Ok(wrap_plain(
                TokenSpecAst::Predefined {
                    name: cap,
                    count: Box::new(count),
                    tapped,
                },
                blocking,
            ));
        }
    }

    // "a number of <desc> tokens equal to <ValueExpr>" (Goblin Gathering, etc.)
    if let Some(after_num) = body.strip_prefix("a number of ")
        && let Some((tok_side, val_raw)) = after_num.split_once(" equal to ")
        && let Ok(eq_val) = parse_value_expr(val_raw, vocab)
        && let Ok(mut inner) = parse_token_creation(&format!("a {}", tok_side.trim()), vocab)
    {
        match &mut inner {
            TokenSpecAst::Creature { count, .. }
            | TokenSpecAst::Predefined { count, .. }
            | TokenSpecAst::Copy { count, .. } => **count = eq_val,
            TokenSpecAst::Role { .. } => {}
        }
        return Ok(wrap_plain(inner, blocking));
    }

    // Prefix-named token: "<Name>, a [legendary] [P/T] <desc> token [with <kws/abilities>]"
    // (e.g. "Voja, a legendary 2/2 green and white Wolf creature token",
    // "Ashaya, the Awoken World, a legendary 4/4 green Elemental creature token",
    // "~'s Notebook, a legendary colorless Book artifact token with \"...\"")
    let (body, prefix_named) = {
        let split_before_tok = body.find(" token").unwrap_or(body.len());
        let search_slice = &body[..split_before_tok];
        if let Some(idx) = search_slice
            .rfind(", a ")
            .or_else(|| search_slice.rfind(", an "))
        {
            let name_part = body[..idx].trim();
            let rest_part = body[idx + 2..].trim();
            if !name_part.is_empty() && !name_part.contains('"') {
                (rest_part, Some(name_part.to_string()))
            } else {
                (body, None)
            }
        } else {
            (body, None)
        }
    };

    // "<count> P/T creature token(s) of the chosen color and type" (Volrath's Laboratory, Riptide Replicator)
    if let Some((before, _)) = body.split_once(" of the chosen color and type")
        && let Ok(mut inner) = parse_token_creation_rich(before.trim(), vocab)
    {
        inner.blocking = inner.blocking || blocking;
        inner.chosen_color = true;
        inner.chosen_type = true;
        return Ok(inner);
    }

    // Standard creature or noncreature token: "<count> [legendary] [<P>/<T>] <desc> token(s) [named <Name>] [with <kws>] [named <Name>]"
    let (core, mut named, mut kw_tail) = if let Some((before, after)) =
        split_once_any(&body, &[" token named ", " tokens named "])
    {
        match after.split_once(" with ") {
            Some((n, kw)) => (before, Some(n.trim().to_string()), Some(kw)),
            None => (before, Some(after.trim().to_string()), None),
        }
    } else if let Some((before, after)) = body.split_once(" token with ") {
        (before, None, Some(after))
    } else if let Some((before, after)) = body.split_once(" tokens with ") {
        (before, None, Some(after))
    } else if let Some(before) = strip_suffix_any(&body, &[" tokens", " token"]) {
        (before, None, None)
    } else {
        return Err(format!("unparsed_token:{raw}"));
    };
    if named.is_none() {
        named = prefix_named;
    }
    if named.is_none()
        && let Some(kt) = kw_tail
        && !kt.contains('"')
        && let Some((kw_only, name_part)) = kt.split_once(" named ")
    {
        kw_tail = Some(kw_only.trim());
        named = Some(name_part.trim().to_string());
    }

    let words: Vec<&str> = core.split_whitespace().collect();
    if words.len() < 2 {
        return Err(format!("unparsed_token_short:{raw}"));
    }

    // Handle "twice that many" / "half that many" / "that many" / "twice x" or single-word count
    let (count_expr, mut cursor) =
        if words.len() >= 4 && words[0] == "twice" && words[1] == "that" && words[2] == "many" {
            (
                ValueExprAst::Mul(vconst(2), Box::new(ValueExprAst::PriorResult)),
                3,
            )
        } else if words.len() >= 4 && words[0] == "half" && words[1] == "that" && words[2] == "many"
        {
            (val_div(ValueExprAst::PriorResult, 2, Rounding::Down), 3)
        } else if words.len() >= 3 && words[0] == "that" && words[1] == "many" {
            (ValueExprAst::PriorResult, 2)
        } else if words.len() >= 3 && words[0] == "twice" && words[1] == "x" {
            (
                ValueExprAst::Mul(vconst(2), Box::new(ValueExprAst::ChosenX)),
                2,
            )
        } else {
            (parse_value_expr(words[0], vocab)?, 1)
        };

    let mut colors = ColorMask::EMPTY;
    let mut supertypes = SupertypeMask::EMPTY;
    let mut card_types = CardTypeMask::EMPTY;
    let mut subtypes = Vec::new();

    // Peel optional leading supertypes (e.g. "a legendary 2/2 ..." or "a legendary colorless Book artifact")
    while cursor < words.len() {
        if let Some(st) = lookup_supertype(words[cursor].trim_matches(',')) {
            supertypes = supertypes.union(st);
            cursor += 1;
        } else {
            break;
        }
    }

    let (power, toughness) = if cursor < words.len()
        && let Some((p, t)) = parse_base_pt(words[cursor])
    {
        cursor += 1;
        (p, t)
    } else {
        (String::new(), String::new())
    };

    for &w in &words[cursor..] {
        let tok = w.trim_matches(',');
        if tok == "and" || tok == "colorless" {
            continue;
        }
        if let Some(c) = lookup_color(tok) {
            colors = colors.union(c);
        } else if let Some(st) = lookup_supertype(tok) {
            supertypes = supertypes.union(st);
        } else if let Some(ct) = lookup_card_type(tok) {
            card_types = card_types.union(ct);
        } else if let Some(sub) = vocab.lookup_subtype(tok) {
            subtypes.push(sub.to_string());
        } else {
            return Err(format!("unparsed_token_desc_tok:{tok}"));
        }
    }
    if power.is_empty() && card_types.is_empty() {
        return Err(format!("unparsed_token_pt:{raw}"));
    }
    if all_colors {
        colors = ColorMask::WHITE
            .union(ColorMask::BLUE)
            .union(ColorMask::BLACK)
            .union(ColorMask::RED)
            .union(ColorMask::GREEN);
    } else if !extra_colors.is_empty() {
        colors = colors.union(extra_colors);
    }
    if card_types.is_empty() {
        card_types = CardTypeMask::CREATURE;
    }

    let mut keywords = Vec::new();
    if let Some(kt) = kw_tail {
        if kt.contains('"') {
            keywords.push(kt.to_string());
        } else if let Some(kws) = parse_keyword_list(kt, vocab) {
            keywords = kws;
        } else {
            return Err(format!("unparsed_token_kw:{kt}"));
        }
    }

    Ok(wrap_plain(
        TokenSpecAst::Creature {
            count: Box::new(match for_each_expr {
                Some(fe) if count_expr != ValueExprAst::Const(1) => {
                    ValueExprAst::Mul(Box::new(count_expr), Box::new(fe))
                }
                Some(fe) => fe,
                None => count_expr,
            }),
            power,
            toughness,
            colors,
            supertypes,
            card_types,
            subtypes,
            keywords,
            tapped,
            attacking,
            named,
        },
        blocking,
    ))
}

/// Split a keyword line on `,` or `;` outside quotes.
#[must_use]
pub fn split_keyword_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for seg in split_top_level(line, ";") {
        for part in split_top_level(seg, ",") {
            if !part.is_empty() {
                out.push(part);
            }
        }
    }
    out
}

pub(super) fn split_named_card_list(names_orig: &str) -> Vec<String> {
    let s = names_orig.trim();
    if let Some((head, last)) = s.rsplit_once(", and ") {
        let mut out: Vec<String> = head
            .split(", ")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if !last.trim().is_empty() {
            out.push(last.trim().to_string());
        }
        return out;
    }
    if s.contains(" and ") {
        return s
            .split(" and ")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
    }
    vec![s.to_string()]
}
