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

pub(crate) fn parse_token_creation(
    ts: &mut TokenStream<'_>,
) -> PResult<(ValueExpr, TokenSpec, EntryModifiers)> {
    // Named legendary token: `Create <Name>, a[n] [legendary] P/T <colors> <subtypes> creature token [with ...]`
    // (`Battle at the Helvault`, `Nissa`, `Jiang Yanggu`, `Darkstar`, `Voja`, etc.)
    {
        let ncp = ts.checkpoint();
        let mut name_words = 0usize;
        while name_words < 6 {
            if ts.peek_word().is_some_and(|w| {
                w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                    || matches!(w.to_ascii_lowercase().as_str(), "the" | "of" | "and")
            }) {
                ts.next_word();
                name_words += 1;
                if ts.eat_comma() {
                    if ts.peek_phrase(&["a", "legendary"]) || ts.peek_phrase(&["an", "legendary"]) {
                        break;
                    }
                    // Could be multi-part title like `Ashaya, the Awoken World, a legendary ...`
                    continue;
                }
            } else {
                break;
            }
        }
        if name_words > 0
            && (ts.peek_phrase(&["a", "legendary"]) || ts.peek_phrase(&["an", "legendary"]))
            && let Ok(res) = crate::v2::parser::try_parse(ts, parse_token_creation)
        {
            return Ok(res);
        }
        ts.restore(ncp);
    }

    let mut deferred_equal_to = false;
    let count = if ts.eat_phrase(&["a", "number", "of"]) {
        deferred_equal_to = true;
        ValueExpr::Const(1)
    } else if ts.eat_phrase(&["that", "many"]) {
        ValueExpr::PriorActionCount(crate::v2::ir::PriorActionKind::ThatMany)
    } else if ts.eat_phrase(&["an", "additional"]) || ts.eat_word("a") || ts.eat_word("an") {
        ValueExpr::Const(1)
    } else {
        parse_value_expr(ts)?
    };
    let leading_tapped_and_attacking = ts.eat_phrase(&["tapped", "and", "attacking"]);
    let leading_tapped = leading_tapped_and_attacking || ts.eat_word("tapped");

    // Copy token: `tokens that are copies of <src> [and that are tapped and attacking] [, except ...]`
    if (ts.eat_word("token") || ts.eat_word("tokens"))
        && (ts.eat_phrase(&["that", "are", "copies", "of"])
            || ts.eat_phrase(&["that's", "a", "copy", "of"]))
    {
        let source = parse_subject_ref(ts)?;
        let mut modifiers = EntryModifiers {
            tapped: leading_tapped,
            attacking: leading_tapped_and_attacking,
            ..EntryModifiers::default()
        };
        if ts.eat_phrase(&["and", "that", "are", "tapped", "and", "attacking"])
            || ts.eat_phrase(&["that's", "tapped", "and", "attacking"])
        {
            modifiers.tapped = true;
            modifiers.attacking = true;
        }
        let exceptions = parse_copy_exceptions(ts);
        let final_count = if deferred_equal_to && ts.eat_phrase(&["equal", "to"]) {
            parse_value_expr(ts)?
        } else if let Some(fe) = opt(ts, parse_for_each_expr) {
            if matches!(count, ValueExpr::Const(1)) {
                fe
            } else {
                ValueExpr::Mul(Box::new(count), Box::new(fe))
            }
        } else {
            count
        };
        return Ok((
            final_count,
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
                "tarmogoyf" => Some(PredefinedToken::Tarmogoyf),
                "heartwood" => Some(PredefinedToken::Heartwood),
                "vibranium" => Some(PredefinedToken::Vibranium),
                "mutavault" => Some(PredefinedToken::Mutavault),
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

    // Creature/artifact token: `[<P>/<T>] [colors] [subtypes...] [card_types...] token(s) [named <Name>] [with <kws|"...">] [that are <colors>]`
    // Also supports `X/X`, `X/1`, `1/X` dynamic P/T tokens and non-P/T tokens (`black Vampire creature token`, `colorless artifact token named Lotus`)!
    let (power_expr, toughness_expr) = if let Some((p, t)) = ts.next_base_pt() {
        (Some(ValueExpr::Const(p)), Some(ValueExpr::Const(t)))
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
                (Some(pv), Some(tv))
            } else {
                ts.restore(p_cp);
                (None, None)
            }
        } else {
            ts.restore(p_cp);
            (None, None)
        }
    };
    let mut colors = ColorMask::EMPTY;
    let mut card_types = CardTypeMask::EMPTY;
    let mut subtypes = Vec::new();
    let mut saw_token_word = false;
    loop {
        if ts.eat_comma() {
            continue;
        }
        let Some(w) = ts.peek_word() else {
            break;
        };
        let low = w.to_ascii_lowercase();
        if low == "token" || low == "tokens" {
            ts.next_word();
            saw_token_word = true;
            break;
        }
        if low == "and" || low == "colorless" || low == "legendary" {
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
    if !saw_token_word || (power_expr.is_none() && card_types.is_empty() && subtypes.is_empty()) {
        return Err(ts.err("expected P/T or token types before `token`"));
    }
    // Optional `named <Capitalized Name>` (e.g. `create a colorless artifact token named Lotus with "..."`)
    if ts.eat_word("named") {
        let mut words_eaten = 0usize;
        while words_eaten < 4
            && let Some(w) = ts.peek_word()
            && w.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && !w.eq_ignore_ascii_case("with")
        {
            ts.next_word();
            words_eaten += 1;
        }
    }

    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    if ts.eat_word("with") {
        loop {
            let wcp = ts.checkpoint();
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
            } else if let Ok((kws, _)) = crate::v2::parser::try_parse(ts, parse_keyword_list) {
                keywords.extend(kws);
            } else {
                ts.restore(wcp);
                break;
            }
            // Optional `, and` / `,` / `and` before next quoted ability (checkpointed so we never eat `and` belonging to a later clause!)
            let sep_cp = ts.checkpoint();
            let had_comma = ts.eat_comma();
            let had_and = ts.eat_word("and");
            if (had_comma || had_and)
                && (matches!(ts.peek().map(|t| &t.kind), Some(TokenKind::Quoted(_)))
                    || (had_and
                        && ts
                            .peek_word()
                            .is_some_and(|w| w.eq_ignore_ascii_case("equip"))))
            {
                continue;
            }
            ts.restore(sep_cp);
            break;
        }
        if keywords.is_empty() && abilities.is_empty() {
            return Err(ts.err("expected keyword or quoted ability after `with`"));
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

/// Shared strictly-typed parser for `, except ...` copy clauses used across copy tokens,
/// `copy <spell>` effects, and `becomes a copy of <src>` effects.
pub(crate) fn parse_copy_exceptions(ts: &mut TokenStream<'_>) -> Vec<CopyException> {
    let mut exceptions = Vec::new();
    let cp = ts.checkpoint();
    if !(ts.eat_comma() && ts.eat_word("except")) {
        ts.restore(cp);
        return exceptions;
    }
    let _ = ts.eat_word("that");
    loop {
        let item_cp = ts.checkpoint();
        if ts.eat_phrase(&["it", "has", "this", "ability"])
            || ts.eat_phrase(&["they", "have", "this", "ability"])
            || ts.eat_phrase(&["has", "this", "ability"])
            || ts.eat_phrase(&["have", "this", "ability"])
        {
            exceptions.push(CopyException::HasThisAbility);
        } else if ((ts.eat_tilde() && ts.eat_phrase(&["'s", "name", "is"]))
            || ts.eat_phrase(&["its", "name", "is"]))
            && ts.eat_tilde()
        {
            exceptions.push(CopyException::KeepOwnName);
        } else if ts.eat_phrase(&["it", "enters", "with"])
            || ts.eat_phrase(&["they", "enter", "with"])
            || ts.eat_phrase(&["the", "token", "enters", "with"])
        {
            let count =
                if ts.eat_phrase(&["an", "additional"]) || ts.eat_word("a") || ts.eat_word("an") {
                    ValueExpr::Const(1)
                } else {
                    let c = crate::v2::parser::try_parse(ts, parse_value_expr)
                        .unwrap_or(ValueExpr::Const(1));
                    let _ = ts.eat_word("additional");
                    c
                };
            if let Ok(kind) =
                crate::v2::parser::try_parse(ts, crate::v2::filter::parse_counter_kind)
                && (ts.eat_word("counter") || ts.eat_word("counters"))
                && (ts.eat_phrase(&["on", "it"]) || ts.eat_phrase(&["on", "them"]))
            {
                let cond = if ts.eat_word("if") {
                    opt(ts, crate::v2::condition::parse_condition)
                } else {
                    None
                };
                exceptions.push(CopyException::EntersWithCounters {
                    kind,
                    count,
                    condition: cond,
                });
            } else {
                ts.restore(item_cp);
                break;
            }
        } else if ts.eat_phrase(&["it", "has"])
            || ts.eat_phrase(&["they", "have"])
            || ts.eat_phrase(&["the", "token", "has"])
            || ts.eat_phrase(&["the", "tokens", "have"])
            || ts.eat_phrase(&["the", "copy", "has"])
            || ts.eat_phrase(&["the", "copies", "have"])
            || ts.eat_word("has")
            || ts.eat_word("have")
        {
            if ts.eat_tilde() && ts.eat_phrase(&["'s", "other", "abilities"]) {
                exceptions.push(CopyException::KeepOwnOtherAbilities);
            } else if let Some(quoted) = ts.next_quoted() {
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
                    ts.restore(item_cp);
                    break;
                }
            } else if let Ok((kws, _)) = crate::v2::parser::try_parse(ts, parse_keyword_list) {
                if ts.eat_word("if")
                    && let Ok(cond) =
                        crate::v2::parser::try_parse(ts, crate::v2::condition::parse_condition)
                {
                    exceptions.push(CopyException::AddKeywordsConditional {
                        keywords: kws,
                        condition: cond,
                    });
                } else {
                    exceptions.push(CopyException::AddKeywords(kws));
                }
            } else {
                ts.restore(item_cp);
                break;
            }
        } else if ts.eat_phrase(&["it", "isn't", "legendary"])
            || ts.eat_phrase(&["it's", "not", "legendary"])
            || ts.eat_phrase(&["they", "aren't", "legendary"])
            || ts.eat_phrase(&["they're", "not", "legendary"])
            || ts.eat_phrase(&["the", "copy", "isn't", "legendary"])
            || ts.eat_phrase(&["the", "token", "isn't", "legendary"])
            || ts.eat_phrase(&["the", "tokens", "aren't", "legendary"])
            || ts.eat_phrase(&["the", "copies", "aren't", "legendary"])
        {
            exceptions.push(CopyException::NotLegendary);
        } else if ts.eat_phrase(&["it's", "an"])
            || ts.eat_phrase(&["it's", "a"])
            || ts.eat_word("it's")
            || ts.eat_word("they're")
            || ts.eat_phrase(&["is", "an"])
            || ts.eat_phrase(&["is", "a"])
            || ts.eat_phrase(&["the", "token", "is", "an"])
            || ts.eat_phrase(&["the", "token", "is", "a"])
            || ts.eat_phrase(&["the", "token", "is"])
            || ts.eat_phrase(&["the", "copy", "is", "an"])
            || ts.eat_phrase(&["the", "copy", "is", "a"])
            || ts.eat_phrase(&["the", "copy", "is"])
            || ts.eat_phrase(&["the", "tokens", "are"])
            || ts.eat_phrase(&["the", "copies", "are"])
        {
            let pt = ts.next_base_pt();
            let mut colors = ColorMask::EMPTY;
            let mut supertypes = crate::ast::SupertypeMask::EMPTY;
            let mut subtypes = Vec::new();
            let mut types = CardTypeMask::EMPTY;
            while let Some(w) = ts.peek_word() {
                let low = w.to_ascii_lowercase();
                if let Some(c) = lookup_color(&low) {
                    ts.next_word();
                    colors = colors.union(c);
                } else if let Some(st) = crate::vocab::lookup_supertype(&low) {
                    ts.next_word();
                    supertypes = supertypes.union(st);
                } else if let Some(ct) = lookup_card_type(&low) {
                    ts.next_word();
                    types = types.union(ct);
                } else if let Some(sub) = ts.vocab.lookup_subtype(&low) {
                    ts.next_word();
                    subtypes.push(sub.to_string());
                } else if low == "and"
                    && ts.peek_n(1).is_some_and(|t| {
                        matches!(t.kind, TokenKind::Word(nw) if {
                            let nl = nw.to_ascii_lowercase();
                            lookup_color(&nl).is_some()
                                || crate::vocab::lookup_supertype(&nl).is_some()
                                || lookup_card_type(&nl).is_some()
                                || ts.vocab.lookup_subtype(&nl).is_some()
                        })
                    })
                {
                    ts.next_word();
                } else {
                    break;
                }
            }
            let keep_existing_types = ts
                .eat_phrase(&["in", "addition", "to", "its", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "their", "other", "types"])
                || ts.eat_phrase(&["in", "addition", "to", "its", "other", "creature", "types"])
                || ts.eat_phrase(&[
                    "in", "addition", "to", "its", "other", "colors", "and", "types",
                ]);
            if pt.is_some() || !subtypes.is_empty() || !types.is_empty() {
                exceptions.push(CopyException::SetBasePTAndTypes {
                    power: pt.map(|(p, _)| p),
                    toughness: pt.map(|(_, t)| t),
                    subtypes,
                    types,
                    keep_existing_types,
                });
            } else if !colors.is_empty() {
                exceptions.push(CopyException::SetColor(colors));
            } else if !supertypes.is_empty() {
                exceptions.push(CopyException::AddSupertypes {
                    supertypes,
                    keep_existing_types,
                });
            } else {
                ts.restore(item_cp);
                break;
            }
        } else if let Some(quoted) = ts.next_quoted() {
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
                ts.restore(item_cp);
                break;
            }
        } else {
            ts.restore(item_cp);
            break;
        }
        let sep_cp = ts.checkpoint();
        let had_comma = ts.eat_comma();
        let had_and = ts.eat_word("and");
        if !had_comma && !had_and {
            ts.restore(sep_cp);
            break;
        }
    }
    if exceptions.is_empty() {
        ts.restore(cp);
    }
    exceptions
}
