//! Strictly-typed token creation parser (predefined, creature, X/X, and copy tokens) for `v2`.

use crate::FaceInput;
use crate::ast::{CardTypeMask, ColorMask};
use crate::v2::card::compile_face_v2;
use crate::v2::filter::{parse_keyword_list, parse_subject_ref};
use crate::v2::ir::{
    CopyException, EntryModifiers, PredefinedToken, TokenSpec, ValueExpr, VarName,
};
use crate::v2::lexer::TokenKind;
use crate::v2::parser::{PResult, TokenStream, opt};
use crate::v2::value::{parse_for_each_expr, parse_value_expr};
use crate::vocab::{lookup_card_type, lookup_color};

pub(super) fn parse_token_creation(
    ts: &mut TokenStream<'_>,
) -> PResult<(ValueExpr, TokenSpec, EntryModifiers)> {
    let mut deferred_equal_to = false;
    let count = if ts.eat_phrase(&["a", "number", "of"]) {
        deferred_equal_to = true;
        ValueExpr::Const(1)
    } else if ts.eat_word("a") || ts.eat_word("an") {
        ValueExpr::Const(1)
    } else {
        parse_value_expr(ts)?
    };
    let leading_tapped = ts.eat_word("tapped");

    // Copy token: `tokens that are copies of <src> [and that are tapped and attacking] [, except ...]`
    if (ts.eat_word("token") || ts.eat_word("tokens"))
        && (ts.eat_phrase(&["that", "are", "copies", "of"])
            || ts.eat_phrase(&["that's", "a", "copy", "of"]))
    {
        let source = parse_subject_ref(ts)?;
        let mut modifiers = EntryModifiers::default();
        if ts.eat_phrase(&["and", "that", "are", "tapped", "and", "attacking"])
            || ts.eat_phrase(&["that's", "tapped", "and", "attacking"])
        {
            modifiers.tapped = true;
            modifiers.attacking = true;
        }
        let mut exceptions = Vec::new();
        {
            let cp = ts.checkpoint();
            if ts.eat_comma() && ts.eat_word("except") {
                if ts.eat_phrase(&["it", "has", "this", "ability"])
                    || ts.eat_phrase(&["they", "have", "this", "ability"])
                {
                    exceptions.push(CopyException::HasThisAbility);
                } else if ts.eat_phrase(&["it", "has"]) || ts.eat_phrase(&["they", "have"]) {
                    if let Some(quoted) = ts.next_quoted() {
                        let compiled = compile_face_v2(
                            FaceInput {
                                name: "~",
                                type_line: "Permanent",
                                oracle_text: quoted,
                            },
                            ts.vocab,
                        );
                        if compiled.is_ok() {
                            exceptions.push(CopyException::AddAbility(Box::new(compiled)));
                        } else {
                            ts.restore(cp);
                        }
                    } else {
                        let (kws, _) = parse_keyword_list(ts)?;
                        exceptions.push(CopyException::AddKeywords(kws));
                        if ts.eat_word("and")
                            && let Some(quoted) = ts.next_quoted()
                        {
                            let compiled = compile_face_v2(
                                FaceInput {
                                    name: "~",
                                    type_line: "Permanent",
                                    oracle_text: quoted,
                                },
                                ts.vocab,
                            );
                            if compiled.is_ok() {
                                exceptions.push(CopyException::AddAbility(Box::new(compiled)));
                            }
                        }
                    }
                } else if ts.eat_phrase(&["it", "isn't", "legendary"])
                    || ts.eat_phrase(&["they", "aren't", "legendary"])
                {
                    exceptions.push(CopyException::NotLegendary);
                } else if ts.eat_phrase(&["it's", "an"]) || ts.eat_phrase(&["it's", "a"]) {
                    let pt = ts.next_base_pt();
                    let mut subtypes = Vec::new();
                    let mut types = CardTypeMask::EMPTY;
                    while let Some(w) = ts.peek_word() {
                        let low = w.to_ascii_lowercase();
                        if let Some(ct) = lookup_card_type(&low) {
                            ts.next_word();
                            types = types.union(ct);
                        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                            ts.next_word();
                            subtypes.push(sub.to_string());
                        } else {
                            break;
                        }
                    }
                    let keep_existing_types =
                        ts.eat_phrase(&["in", "addition", "to", "its", "other", "types"]);
                    if pt.is_some() || !subtypes.is_empty() || !types.is_empty() {
                        exceptions.push(CopyException::SetBasePTAndTypes {
                            power: pt.map(|(p, _)| p),
                            toughness: pt.map(|(_, t)| t),
                            subtypes,
                            types,
                            keep_existing_types,
                        });
                    } else {
                        ts.restore(cp);
                    }
                } else {
                    ts.restore(cp);
                }
            } else {
                ts.restore(cp);
            }
        }
        return Ok((
            count,
            TokenSpec::CopyOf {
                source: Box::new(source),
                exceptions,
            },
            modifiers,
        ));
    }

    // Predefined tokens (`Treasure token`, `Clue token`, `Wicked Role token`, `Mutagen token`, etc.)
    {
        let cp = ts.checkpoint();
        let predef = if ts.eat_phrase(&["wicked", "role"]) {
            Some(PredefinedToken::WickedRole)
        } else if ts.eat_phrase(&["monster", "role"]) {
            Some(PredefinedToken::MonsterRole)
        } else if ts.eat_phrase(&["royal", "role"]) {
            Some(PredefinedToken::RoyalRole)
        } else if ts.eat_phrase(&["sorcerer", "role"]) {
            Some(PredefinedToken::SorcererRole)
        } else if ts.eat_phrase(&["cursed", "role"]) {
            Some(PredefinedToken::CursedRole)
        } else if ts.eat_phrase(&["young", "hero", "role"]) {
            Some(PredefinedToken::YoungHeroRole)
        } else if let Some(w) = ts.next_word() {
            match w.to_ascii_lowercase().as_str() {
                "treasure" => Some(PredefinedToken::Treasure),
                "clue" => Some(PredefinedToken::Clue),
                "food" => Some(PredefinedToken::Food),
                "blood" => Some(PredefinedToken::Blood),
                "map" => Some(PredefinedToken::Map),
                "powerstone" => Some(PredefinedToken::Powerstone),
                "shard" => Some(PredefinedToken::Shard),
                "lander" => Some(PredefinedToken::Lander),
                "junk" => Some(PredefinedToken::Junk),
                "gold" => Some(PredefinedToken::Gold),
                "incubator" => Some(PredefinedToken::Incubator),
                "role" => Some(PredefinedToken::Role),
                "walker" => Some(PredefinedToken::Walker),
                "mutagen" => Some(PredefinedToken::Mutagen),
                _ => None,
            }
        } else {
            None
        };
        if let Some(pt) = predef
            && (ts.eat_word("token") || ts.eat_word("tokens"))
        {
            let mut mods = EntryModifiers {
                tapped: leading_tapped,
                ..EntryModifiers::default()
            };
            if ts.eat_word("tapped") {
                mods.tapped = true;
            }
            if ts.eat_phrase(&["attached", "to"]) {
                mods.attached_to = Some(Box::new(parse_subject_ref(ts)?));
            }
            let final_count = if deferred_equal_to && ts.eat_phrase(&["equal", "to"]) {
                parse_value_expr(ts)?
            } else if let Some(fe) = opt(ts, parse_for_each_expr) {
                if count == ValueExpr::Const(1) {
                    fe
                } else {
                    ValueExpr::Mul(Box::new(count), Box::new(fe))
                }
            } else {
                count
            };
            return Ok((final_count, TokenSpec::Predefined(pt), mods));
        }
        ts.restore(cp);
    }

    // Creature token: `<P>/<T> [colors] [subtypes...] [card_types...] token(s) [with <kws>] [that are <colors>]`
    // Also supports `X/X`, `X/1`, `1/X` dynamic P/T tokens!
    let (power_expr, toughness_expr) = if let Some((p, t)) = ts.next_base_pt() {
        (ValueExpr::Const(p), ValueExpr::Const(t))
    } else {
        let p_cp = ts.checkpoint();
        let p_val = if ts.eat_word("x") {
            Some(ValueExpr::Var(VarName::X))
        } else {
            ts.next_number(false).map(ValueExpr::Const)
        };
        let slash_ok = matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Slash));
        if let Some(pv) = p_val
            && slash_ok
        {
            ts.next_tok();
            let t_val = if ts.eat_word("x") {
                Some(ValueExpr::Var(VarName::X))
            } else {
                ts.next_number(false).map(ValueExpr::Const)
            };
            if let Some(tv) = t_val {
                (pv, tv)
            } else {
                ts.restore(p_cp);
                return Err(ts.err("expected P/T or predefined token name"));
            }
        } else {
            ts.restore(p_cp);
            return Err(ts.err("expected P/T or predefined token name"));
        }
    };
    let mut colors = ColorMask::EMPTY;
    let mut card_types = CardTypeMask::EMPTY;
    let mut subtypes = Vec::new();
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if low == "token" || low == "tokens" {
            ts.next_word();
            break;
        }
        if low == "and" || low == "colorless" {
            ts.next_word();
            continue;
        }
        if let Some(c) = lookup_color(&low) {
            ts.next_word();
            colors = colors.union(c);
        } else if let Some(ct) = lookup_card_type(&low) {
            ts.next_word();
            card_types = card_types.union(ct);
        } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
            ts.next_word();
            subtypes.push(sub.to_string());
        } else {
            return Err(ts.err(format!("unexpected token descriptor `{w}`")));
        }
    }

    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    if ts.eat_word("with") {
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_face_v2(
                FaceInput {
                    name: "~",
                    type_line: "Creature",
                    oracle_text: quoted,
                },
                ts.vocab,
            );
            if !compiled.is_ok() {
                return Err(compiled
                    .errors
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| ts.err("failed to compile token ability text")));
            }
            abilities.push(compiled);
        } else {
            let (kws, _) = parse_keyword_list(ts)?;
            keywords = kws;
            if ts.eat_word("and")
                && let Some(quoted) = ts.next_quoted()
            {
                let compiled = compile_face_v2(
                    FaceInput {
                        name: "~",
                        type_line: "Creature",
                        oracle_text: quoted,
                    },
                    ts.vocab,
                );
                if !compiled.is_ok() {
                    return Err(compiled
                        .errors
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| ts.err("failed to compile token ability text")));
                }
                abilities.push(compiled);
            }
        }
    }

    // Trailing `that are blue, black, and red`
    if ts.eat_phrase(&["that", "are"]) || ts.eat_phrase(&["that's"]) {
        loop {
            let _ = ts.eat_comma();
            let _ = ts.eat_word("and");
            if let Some(w) = ts.peek_word()
                && let Some(c) = lookup_color(&w.to_ascii_lowercase())
            {
                ts.next_word();
                colors = colors.union(c);
                continue;
            }
            break;
        }
    }

    let mut mods = EntryModifiers {
        tapped: leading_tapped,
        ..EntryModifiers::default()
    };
    if ts.eat_phrase(&["tapped", "and", "attacking"]) {
        mods.tapped = true;
        mods.attacking = true;
    } else if ts.eat_word("tapped") {
        mods.tapped = true;
    }
    let final_count = if deferred_equal_to && ts.eat_phrase(&["equal", "to"]) {
        parse_value_expr(ts)?
    } else if let Some(fe) = opt(ts, parse_for_each_expr) {
        if count == ValueExpr::Const(1) {
            fe
        } else {
            ValueExpr::Mul(Box::new(count), Box::new(fe))
        }
    } else {
        count
    };

    Ok((
        final_count,
        TokenSpec::Creature {
            power: power_expr,
            toughness: toughness_expr,
            colors,
            card_types,
            subtypes,
            keywords,
            abilities,
        },
        mods,
    ))
}
