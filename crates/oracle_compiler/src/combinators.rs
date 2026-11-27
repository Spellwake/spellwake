//! Shared compositional parsing combinators and lightweight AST constructors used across
//! `filter/` and `clauses/`.
//!
//! Centralizes punctuation trimming, balanced-quote extraction, recursion/operation budgeting,
//! common noun/step/rounding/visibility/color parsers, and small AST filter constructors so
//! submodules stay focused on Magic grammar rules rather than repeating boilerplate.

use std::cell::Cell;

use crate::ast::{CardTypeMask, ObjectFilterAst, PileVisibilityAst, PlayerRefAst, Rounding};
use crate::filter::parse_player_ref;

/// Trim surrounding whitespace and trailing clause punctuation (`.`, `,`, `;`).
#[inline]
pub(crate) fn trim_punct(s: &str) -> &str {
    s.trim().trim_end_matches(['.', ',', ';'])
}

thread_local! {
    static PARSE_DEPTH: Cell<u32> = const { Cell::new(0) };
    static PARSE_OPS: Cell<u32> = const { Cell::new(0) };
}

/// RAII recursion-depth and operation-budget guard preventing runaway backtracking in recursive
/// clause combinators.
pub(crate) struct DepthGuard;

impl DepthGuard {
    /// Current recursion depth on this thread.
    #[inline]
    pub(crate) fn current_depth() -> u32 {
        PARSE_DEPTH.with(Cell::get)
    }

    /// Enter one recursive parsing frame; returns `None` when depth or op budget is exhausted.
    #[inline]
    pub(crate) fn enter() -> Option<Self> {
        PARSE_DEPTH.with(|d| {
            let cur = d.get();
            if cur == 0 {
                PARSE_OPS.with(|ops| ops.set(1));
                d.set(1);
                return Some(Self);
            }
            if cur >= 7 {
                return None;
            }
            let allowed = PARSE_OPS.with(|ops| {
                let n = ops.get();
                if n >= 800 {
                    false
                } else {
                    ops.set(n + 1);
                    true
                }
            });
            if !allowed {
                return None;
            }
            d.set(cur + 1);
            Some(Self)
        })
    }
}

impl Drop for DepthGuard {
    #[inline]
    fn drop(&mut self) {
        PARSE_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Extract all balanced double-quoted spans from `text`, plus any non-quoted prefix before the
/// first quote and trailing suffix after the last quote. Returns `None` if quotes are unbalanced
/// or absent.
pub(crate) fn extract_quoted_spans(text: &str) -> Option<(&str, Vec<&str>, &str)> {
    let mut spans = Vec::new();
    let mut first_open: Option<usize> = None;
    let mut open: Option<usize> = None;
    let mut last_close: Option<usize> = None;
    for (idx, ch) in text.char_indices() {
        if ch == '"' {
            if let Some(start) = open.take() {
                spans.push(&text[start + 1..idx]);
                last_close = Some(idx);
            } else {
                if first_open.is_none() {
                    first_open = Some(idx);
                }
                open = Some(idx);
            }
        }
    }
    if open.is_some() || spans.is_empty() {
        return None;
    }
    let before = &text[..first_open?];
    let after = &text[last_close? + 1..];
    Some((before, spans, after))
}

/// Convenience constructor for an [`ObjectFilterAst`] matching one or more card types.
#[inline]
pub(crate) fn obj_type(mask: CardTypeMask) -> ObjectFilterAst {
    ObjectFilterAst {
        any_types: mask,
        ..Default::default()
    }
}

/// Convenience constructor for an [`ObjectFilterAst`] matching any permanent.
#[inline]
pub(crate) fn any_perm() -> ObjectFilterAst {
    ObjectFilterAst {
        any_permanent: true,
        ..Default::default()
    }
}

/// Convenience constructor for an [`ObjectFilterAst`] matching any card.
#[inline]
pub(crate) fn any_card_flt() -> ObjectFilterAst {
    ObjectFilterAst {
        any_card: true,
        ..Default::default()
    }
}

/// Parse a rounding direction (`up` / `down`) into [`Rounding`].
#[inline]
pub(crate) fn parse_rounding_word(word: &str) -> Option<Rounding> {
    match word.trim() {
        "up" => Some(Rounding::Up),
        "down" => Some(Rounding::Down),
        _ => None,
    }
}

/// Parse a pile visibility modifier (`face-up` / `face-down`) into [`PileVisibilityAst`].
#[inline]
pub(crate) fn parse_pile_visibility(word: &str) -> Option<PileVisibilityAst> {
    match word.trim() {
        "face-down" | "face down" => Some(PileVisibilityAst::FaceDown),
        "face-up" | "face up" => Some(PileVisibilityAst::FaceUp),
        _ => None,
    }
}

/// Parse `a "<l1>" pile and a "<l2>" pile` into `[l1, l2]`.
pub(crate) fn parse_two_quoted_piles(raw: &str) -> Option<Vec<String>> {
    let (p1, p2) = raw.split_once(" pile and ")?;
    let l1 = p1
        .trim()
        .strip_prefix("a \"")
        .or_else(|| p1.trim().strip_prefix("an \""))?
        .strip_suffix('"')?;
    let l2 = p2
        .trim()
        .strip_prefix("a \"")
        .or_else(|| p2.trim().strip_prefix("an \""))?
        .strip_suffix("\" pile")?;
    Some(vec![l1.to_string(), l2.to_string()])
}

/// Parse `"<l1>" or "<l2>"` (or multi-label quoted disjunction) into label strings.
pub(crate) fn parse_quoted_label_or_list(raw: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = raw
        .split(" or ")
        .map(|p| p.trim().trim_matches('"').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() >= 2 && raw.contains('"') {
        Some(parts)
    } else {
        None
    }
}

/// Normalize sticker kind wording into canonical AST tags.
#[inline]
pub(crate) fn normalize_sticker_kind(raw: &str) -> String {
    match raw.trim() {
        "name" => "name".to_string(),
        "art" => "art".to_string(),
        "ability" => "ability".to_string(),
        "power and toughness" => "power_and_toughness".to_string(),
        other => other.to_string(),
    }
}

/// Parse a player reference including anaphoric `"the player"`.
#[inline]
pub(crate) fn parse_player_ref_or_the_player(raw: &str) -> Option<PlayerRefAst> {
    match raw.trim() {
        "the player" => Some(PlayerRefAst::ThatPlayer),
        other => parse_player_ref(other),
    }
}

/// Strip a leading `"then "` from both lowercase and original-case slices, keeping them aligned.
#[inline]
pub(crate) fn strip_leading_then<'a>(s: &'a str, low: &'a str) -> Option<(&'a str, &'a str)> {
    let rest_low = low.strip_prefix("then ")?;
    let rest_s = s.get(s.len().saturating_sub(rest_low.len())..)?;
    Some((rest_s, rest_low))
}

/// Split on the first matching delimiter from a slice of candidate delimiters.
#[inline]
pub(crate) fn split_once_any<'a>(s: &'a str, needles: &[&str]) -> Option<(&'a str, &'a str)> {
    for needle in needles {
        if let Some(pair) = s.split_once(needle) {
            return Some(pair);
        }
    }
    None
}
