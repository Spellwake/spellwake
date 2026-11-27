//! Top-level card and face compiler for `v2`.
//!
//! Strips multi-line reminder text, normalizes card self-references to `~`, dispatches each
//! paragraph by syntactic structure (Saga chapters, Planeswalker loyalty, Modal/Spree blocks,
//! Activated abilities, Triggered abilities, Keyword lines, Replacements, Statics, Spell effects),
//! and records every unparsed clause as an explicit fail-closed [`ParseError`].

use crate::preprocess::{
    BULLET, EM_DASH, normalize_self, split_cost_and_effect, split_loyalty_line, split_saga_line,
    strip_reminder, strip_reminder_multiline,
};
use crate::v2::clause::{parse_effect_body_str, parse_effect_sentence};
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::parse_keyword_list;
use crate::v2::ir::{
    Ability, ChoiceCount, ChoiceKind, CompiledCardV2, CompiledFaceV2, Effect, LoyaltyDelta,
    ModalMode, ParseError, ParseStage, PlayerRef, Span, SpreeMode,
};
use crate::v2::lexer::{TokenKind, tokenize};
use crate::v2::parser::TokenStream;
use crate::v2::replacement::parse_replacement;
use crate::v2::static_ability::parse_static_ability;
use crate::v2::trigger::parse_trigger_header;
use crate::{CardInput, FaceInput, Vocabulary};

/// Compile a multi-face card using the strictly-typed, fail-closed `v2` parser.
#[must_use]
pub fn compile_card_v2(input: &CardInput<'_>, vocab: &Vocabulary) -> CompiledCardV2 {
    let faces: Vec<CompiledFaceV2> = input
        .faces
        .iter()
        .map(|f| compile_face_v2(*f, vocab))
        .collect();
    let strictly_compiled = faces.iter().all(CompiledFaceV2::is_ok);
    CompiledCardV2 {
        oracle_id: input.oracle_id.to_string(),
        name: input.name.to_string(),
        layout: input.layout.to_string(),
        faces,
        strictly_compiled,
    }
}

/// Compile one card face using the strictly-typed, fail-closed `v2` parser.
#[must_use]
pub fn compile_face_v2(face: FaceInput<'_>, vocab: &Vocabulary) -> CompiledFaceV2 {
    let mut out = CompiledFaceV2 {
        name: face.name.to_string(),
        type_line: face.type_line.to_string(),
        ..CompiledFaceV2::default()
    };
    let is_spell = (face.type_line.contains("Instant") || face.type_line.contains("Sorcery"))
        && !(face.type_line.contains("Creature")
            || face.type_line.contains("Artifact")
            || face.type_line.contains("Enchantment")
            || face.type_line.contains("Land")
            || face.type_line.contains("Planeswalker"));
    let is_planeswalker = face.type_line.contains("Planeswalker");
    let is_saga = face.type_line.contains("Saga");

    let stripped_full = strip_reminder_multiline(face.oracle_text);
    let lines: Vec<String> = stripped_full
        .split('\n')
        .map(strip_reminder)
        .filter(|l| !l.is_empty())
        .map(|l| normalize_self(&l, face.name, face.type_line))
        .collect();

    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];

        // 1. Modal block (`Choose two —` followed by `• ...` lines)
        if let Some(count) = parse_modal_header(line) {
            let mut modes = Vec::new();
            i += 1;
            while i < lines.len() && lines[i].starts_with(BULLET) {
                let mode_text = lines[i].trim_start_matches(BULLET).trim();
                match parse_effect_body_str(mode_text, vocab) {
                    Ok(body) => modes.push(ModalMode { body }),
                    Err(errs) => out.errors.extend(errs),
                }
                i += 1;
            }
            if modes.is_empty() {
                out.errors.push(ParseError {
                    span: Span {
                        start: 0,
                        end: line.len(),
                    },
                    stage: ParseStage::Effect,
                    message: "modal header with zero modes".to_string(),
                    text: line.clone(),
                });
            } else {
                out.spell_effects.push(Effect::PromptChoice {
                    chooser: PlayerRef::You,
                    kind: ChoiceKind::Modal { count, modes },
                });
            }
            continue;
        }

        // 2. Spree block (`Spree` followed by `+ {cost} — <body>` lines)
        if line.eq_ignore_ascii_case("spree") {
            if let Ok(toks) = tokenize(line) {
                let mut ts = TokenStream::new(line, &toks, vocab, ParseStage::Face);
                if let Ok((kws, _)) = parse_keyword_list(&mut ts) {
                    out.keywords.extend(kws);
                }
            }
            let mut spree_modes = Vec::new();
            i += 1;
            while i < lines.len() && lines[i].starts_with("+ ") {
                if let Some((cost_raw, body_raw)) = lines[i]["+ ".len()..].split_once(EM_DASH) {
                    let cost_res = tokenize(cost_raw.trim()).and_then(|toks| {
                        let mut ts =
                            TokenStream::new(cost_raw.trim(), &toks, vocab, ParseStage::Cost);
                        let c = parse_cost_list(&mut ts)?;
                        ts.expect_eof()?;
                        Ok(c)
                    });
                    let body_res = parse_effect_body_str(body_raw.trim(), vocab);
                    match (cost_res, body_res) {
                        (Ok(cost), Ok(body)) => spree_modes.push(SpreeMode { cost, body }),
                        (Err(e), _) => out.errors.push(e),
                        (_, Err(errs)) => out.errors.extend(errs),
                    }
                } else {
                    out.errors.push(ParseError {
                        span: Span {
                            start: 0,
                            end: lines[i].len(),
                        },
                        stage: ParseStage::Effect,
                        message: "malformed Spree mode line".to_string(),
                        text: lines[i].clone(),
                    });
                }
                i += 1;
            }
            if !spree_modes.is_empty() {
                out.spell_effects.push(Effect::PromptChoice {
                    chooser: PlayerRef::You,
                    kind: ChoiceKind::Spree { modes: spree_modes },
                });
            }
            continue;
        }

        // 3. Saga chapter line (`I — ...`, `II — ...`, `III — ...`)
        if is_saga && let Some((chapters_str, body_str)) = split_saga_line(line) {
            let chapters: Vec<u8> = chapters_str.iter().filter_map(|r| roman_to_u8(r)).collect();
            match parse_effect_body_str(body_str, vocab) {
                Ok(body) => out.abilities.push(Ability::SagaChapter { chapters, body }),
                Err(errs) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 4. Planeswalker loyalty line (`+1: ...`, `-2: ...`, `-7: ...`)
        if is_planeswalker && let Some((delta_str, body_str)) = split_loyalty_line(line) {
            let delta = parse_loyalty_delta(&delta_str);
            match (delta, parse_effect_body_str(body_str, vocab)) {
                (Some(d), Ok(body)) => out.abilities.push(Ability::Loyalty { delta: d, body }),
                (None, _) => out.errors.push(ParseError {
                    span: Span {
                        start: 0,
                        end: delta_str.len(),
                    },
                    stage: ParseStage::Cost,
                    message: format!("invalid loyalty delta `{delta_str}`"),
                    text: line.clone(),
                }),
                (_, Err(errs)) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 5. Activated ability (`<cost>: <body>`)
        if !is_spell && let Some((cost_str, body_str)) = split_cost_and_effect(line) {
            let cost_res = tokenize(cost_str).and_then(|toks| {
                let mut ts = TokenStream::new(cost_str, &toks, vocab, ParseStage::Cost);
                let c = parse_cost_list(&mut ts)?;
                ts.expect_eof()?;
                Ok(c)
            });
            match (cost_res, parse_effect_body_str(body_str, vocab)) {
                (Ok(cost), Ok(mut body)) => {
                    let mut restrictions = Vec::new();
                    body.retain(|eff| match eff {
                        Effect::Restriction(rs) => {
                            restrictions.extend(rs.iter().copied());
                            false
                        }
                        _ => true,
                    });
                    out.abilities.push(Ability::Activated {
                        cost,
                        body,
                        restrictions,
                    });
                }
                (Err(e), _) => out.errors.push(e),
                (_, Err(errs)) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 6. Triggered ability (`When/Whenever/At the beginning of ..., <body>`)
        if !is_spell && is_trigger_line(line) {
            match parse_triggered_line(line, vocab) {
                Ok(ab) => out.abilities.push(ab),
                Err(errs) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 7. Standalone keyword line (`Flying, vigilance`)
        if let Ok(toks) = tokenize(line) {
            let mut ts = TokenStream::new(line, &toks, vocab, ParseStage::Face);
            if let Ok((kws, _)) = parse_keyword_list(&mut ts)
                && ts.is_eof()
            {
                out.keywords.extend(kws);
                i += 1;
                continue;
            }
        }

        // 8. Spell effects on Instant/Sorcery
        if is_spell {
            match parse_effect_body_str(line, vocab) {
                Ok(effs) => out.spell_effects.extend(effs),
                Err(errs) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 9. Replacement or Static ability on permanent/emblem
        match tokenize(line) {
            Ok(toks) => {
                let mut rep_ts = TokenStream::new(line, &toks, vocab, ParseStage::Replacement);
                if let Ok(rep) = parse_replacement(&mut rep_ts) {
                    out.replacements.push(rep);
                    i += 1;
                    continue;
                }
                let mut st_ts = TokenStream::new(line, &toks, vocab, ParseStage::Static);
                match parse_static_ability(&mut st_ts) {
                    Ok(st) => out.statics.push(st),
                    Err(e) => out.errors.push(e),
                }
            }
            Err(e) => out.errors.push(e),
        }
        i += 1;
    }

    out
}

fn parse_modal_header(line: &str) -> Option<ChoiceCount> {
    let low = line.trim().to_ascii_lowercase();
    let head = low.strip_suffix(EM_DASH)?.trim();
    match head {
        "choose one" => Some(ChoiceCount::Exact(1)),
        "choose two" => Some(ChoiceCount::Exact(2)),
        "choose three" => Some(ChoiceCount::Exact(3)),
        "choose up to two" => Some(ChoiceCount::UpTo(2)),
        "choose any number" => Some(ChoiceCount::AnyNumber),
        _ => None,
    }
}

fn roman_to_u8(s: &str) -> Option<u8> {
    match s.trim() {
        "I" => Some(1),
        "II" => Some(2),
        "III" => Some(3),
        "IV" => Some(4),
        "V" => Some(5),
        "VI" => Some(6),
        _ => None,
    }
}

fn parse_loyalty_delta(s: &str) -> Option<LoyaltyDelta> {
    let norm = s.trim().replace('\u{2212}', "-");
    if norm.eq_ignore_ascii_case("-x") {
        return Some(LoyaltyDelta::MinusX);
    }
    norm.parse::<i16>().ok().map(LoyaltyDelta::Fixed)
}

fn is_trigger_line(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    low.starts_with("when ")
        || low.starts_with("whenever ")
        || low.starts_with("at the beginning of ")
}

fn parse_triggered_line(line: &str, vocab: &Vocabulary) -> Result<Ability, Vec<ParseError>> {
    let toks = tokenize(line).map_err(|e| vec![e])?;
    let mut ts = TokenStream::new(line, &toks, vocab, ParseStage::Trigger);
    let trigger = parse_trigger_header(&mut ts).map_err(|e| vec![e])?;
    if !ts.eat_comma() {
        return Err(vec![ts.err("expected `,` after trigger header")]);
    }
    // Parse remaining tokens as one or more sentences (split on `.` outside quotes, which are
    // already tokenized as single `TokenKind::Quoted` tokens!)
    let mut body = Vec::new();
    let mut cur_start = ts.pos;
    for idx in ts.pos..toks.len() {
        if matches!(toks[idx].kind, TokenKind::Period) {
            if idx > cur_start {
                let mut sub =
                    TokenStream::new(line, &toks[cur_start..idx], vocab, ParseStage::Effect);
                let effs = parse_effect_sentence(&mut sub).map_err(|e| vec![e])?;
                body.extend(effs);
            }
            cur_start = idx + 1;
        }
    }
    if cur_start < toks.len() {
        let mut sub = TokenStream::new(line, &toks[cur_start..], vocab, ParseStage::Effect);
        let effs = parse_effect_sentence(&mut sub).map_err(|e| vec![e])?;
        body.extend(effs);
    }
    Ok(Ability::Triggered { trigger, body })
}
