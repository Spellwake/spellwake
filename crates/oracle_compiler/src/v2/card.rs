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
    ModalMode, ParseError, ParseStage, PlayerRef, Span, SpreeMode, StickerPayload, TableRowPayload,
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
        let raw_line = &lines[i];
        let line_owned = strip_ability_word_prefix(raw_line);
        let line = line_owned.as_str();

        // 0a. Sticker row (`{TK}{TK} — 1/5` or `{TK}{TK}{TK} — Whenever ~ attacks, ...`)
        if let Some((ticket_cost, rhs)) = split_sticker_row(line) {
            if let Some((p, t)) = parse_bare_base_pt(rhs) {
                out.abilities.push(Ability::StickerRow {
                    ticket_cost,
                    payload: StickerPayload::BasePt(p, t),
                });
            } else {
                let compiled = compile_face_v2(
                    FaceInput {
                        name: face.name,
                        type_line: "Permanent",
                        oracle_text: rhs,
                    },
                    vocab,
                );
                if compiled.is_ok() {
                    out.abilities.push(Ability::StickerRow {
                        ticket_cost,
                        payload: StickerPayload::Face(Box::new(compiled)),
                    });
                } else {
                    out.errors.extend(compiled.errors);
                }
            }
            i += 1;
            continue;
        }

        // 0b. Die-roll table / Station threshold row (`1—9 | ...`, `20 | ...`, `9+ | Flying`)
        if let Some((min, max, open_ended, rhs_raw)) = split_pipe_table_row(line) {
            let rhs_owned = strip_ability_word_prefix(rhs_raw);
            let rhs = rhs_owned.as_str();
            if is_spell || !open_ended {
                match parse_effect_body_str(rhs, vocab) {
                    Ok(effs) => out.abilities.push(Ability::TableRow {
                        min,
                        max,
                        open_ended,
                        payload: TableRowPayload::Effects(effs),
                    }),
                    Err(errs) => out.errors.extend(errs),
                }
            } else {
                let compiled = compile_face_v2(
                    FaceInput {
                        name: face.name,
                        type_line: face.type_line,
                        oracle_text: rhs,
                    },
                    vocab,
                );
                if compiled.is_ok() {
                    out.abilities.push(Ability::TableRow {
                        min,
                        max,
                        open_ended,
                        payload: TableRowPayload::Face(Box::new(compiled)),
                    });
                } else if let Ok(effs) = parse_effect_body_str(rhs, vocab) {
                    out.abilities.push(Ability::TableRow {
                        min,
                        max,
                        open_ended,
                        payload: TableRowPayload::Effects(effs),
                    });
                } else {
                    out.errors.extend(compiled.errors);
                }
            }
            i += 1;
            continue;
        }

        // 0c. Case enchantment rows (`To solve — <cond>` / `Solved — <ability>`)
        if let Some(cond_str) = line.strip_prefix("To solve \u{2014} ") {
            let cond_trimmed = cond_str.trim().trim_end_matches('.');
            match tokenize(cond_trimmed).and_then(|toks| {
                let mut ts = TokenStream::new(cond_trimmed, &toks, vocab, ParseStage::Static);
                let c = crate::v2::condition::parse_condition(&mut ts)?;
                ts.expect_eof()?;
                Ok(c)
            }) {
                Ok(c) => out.abilities.push(Ability::CaseSolveCondition(c)),
                Err(e) => out.errors.push(e),
            }
            i += 1;
            continue;
        }
        if let Some(solved_rhs) = line.strip_prefix("Solved \u{2014} ") {
            let compiled = compile_face_v2(
                FaceInput {
                    name: face.name,
                    type_line: face.type_line,
                    oracle_text: solved_rhs.trim(),
                },
                vocab,
            );
            if compiled.is_ok() {
                out.abilities.push(Ability::CaseSolved(Box::new(compiled)));
            } else {
                out.errors.extend(compiled.errors);
            }
            i += 1;
            continue;
        }

        // 0. Spell-or-permanent static rules (`As an additional cost to cast ...`, `This spell/~ costs ...`,
        //    `This spell can't be countered`, `You may <cost> rather than pay ...`), including multi-sentence static rules
        //    like `This spell costs {1} less to cast ... This effect reduces only the amount of colored mana you pay.`
        let low_line = line.to_ascii_lowercase();
        if (low_line.starts_with("as an additional cost to cast this spell,")
            || low_line.starts_with("as an additional cost to cast ~,")
            || low_line.starts_with("this spell costs ")
            || low_line.starts_with("~ costs ")
            || low_line.starts_with("this spell can't be countered")
            || low_line.starts_with("~ can't be countered")
            || low_line.contains("as though it had flash")
            || low_line.contains("rather than pay"))
            && let Ok(toks) = tokenize(line)
        {
            let mut st_ts = TokenStream::new(line, &toks, vocab, ParseStage::Static);
            if let Ok(st) = parse_static_ability(&mut st_ts) {
                out.statics.push(st);
                i += 1;
                continue;
            }
            if let Some(first_dot) = toks
                .iter()
                .position(|t| matches!(t.kind, TokenKind::Period))
                && first_dot + 1 < toks.len()
            {
                let mut head_ts =
                    TokenStream::new(line, &toks[..first_dot], vocab, ParseStage::Static);
                let mut tail_ts =
                    TokenStream::new(line, &toks[first_dot + 1..], vocab, ParseStage::Static);
                if let (Ok(st1), Ok(st2)) = (
                    parse_static_ability(&mut head_ts),
                    parse_static_ability(&mut tail_ts),
                ) {
                    out.statics.push(st1);
                    out.statics.push(st2);
                    i += 1;
                    continue;
                }
            }
        }

        // 1. Modal block (`Choose one —`, `Choose one. If ..., choose ... instead.` followed by `• ...` lines)
        let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
        if let Some(count) = parse_modal_header(line, next_is_bullet) {
            let mut modes = Vec::new();
            i += 1;
            while i < lines.len() && lines[i].starts_with(BULLET) {
                let raw_mode = lines[i].trim_start_matches(BULLET).trim();
                let mode_clean = strip_ability_word_prefix(raw_mode);
                match parse_effect_body_str(&mode_clean, vocab) {
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
                    text: line.to_string(),
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

        // 3. Saga chapter line (`I — ...`, `II — ...`, `III — ...`, including inline modal `I, II — Choose one —`)
        if is_saga && let Some((chapters_str, body_str)) = split_saga_line(line) {
            let chapters: Vec<u8> = chapters_str.iter().filter_map(|r| roman_to_u8(r)).collect();
            let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
            if let Some(count) = parse_modal_header(body_str, next_is_bullet) {
                let mut modes = Vec::new();
                i += 1;
                while i < lines.len() && lines[i].starts_with(BULLET) {
                    let raw_mode = lines[i].trim_start_matches(BULLET).trim();
                    let mode_clean = strip_ability_word_prefix(raw_mode);
                    match parse_effect_body_str(&mode_clean, vocab) {
                        Ok(body) => modes.push(ModalMode { body }),
                        Err(errs) => out.errors.extend(errs),
                    }
                    i += 1;
                }
                if !modes.is_empty() {
                    out.abilities.push(Ability::SagaChapter {
                        chapters,
                        body: vec![Effect::PromptChoice {
                            chooser: PlayerRef::You,
                            kind: ChoiceKind::Modal { count, modes },
                        }],
                    });
                }
                continue;
            }
            match parse_effect_body_str(body_str, vocab) {
                Ok(body) => out.abilities.push(Ability::SagaChapter { chapters, body }),
                Err(errs) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 4. Planeswalker loyalty line (`+1: ...`, `-2: ...`, `-7: ...`, including granted loyalty abilities on Talents/permanents and inline modals!)
        if !is_spell && let Some((delta_str, body_str)) = split_loyalty_line(line) {
            let delta = parse_loyalty_delta(&delta_str);
            let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
            if let Some(count) = parse_modal_header(body_str, next_is_bullet) {
                let mut modes = Vec::new();
                i += 1;
                while i < lines.len() && lines[i].starts_with(BULLET) {
                    let raw_mode = lines[i].trim_start_matches(BULLET).trim();
                    let mode_text = strip_ability_word_prefix(raw_mode);
                    match parse_effect_body_str(&mode_text, vocab) {
                        Ok(body) => modes.push(ModalMode { body }),
                        Err(errs) => out.errors.extend(errs),
                    }
                    i += 1;
                }
                match delta {
                    Some(d) if !modes.is_empty() => out.abilities.push(Ability::Loyalty {
                        delta: d,
                        body: vec![Effect::PromptChoice {
                            chooser: PlayerRef::You,
                            kind: ChoiceKind::Modal { count, modes },
                        }],
                    }),
                    Some(_) => out.errors.push(ParseError {
                        span: Span {
                            start: 0,
                            end: line.len(),
                        },
                        stage: ParseStage::Effect,
                        message: "modal loyalty ability with zero modes".to_string(),
                        text: line.to_string(),
                    }),
                    None => out.errors.push(ParseError {
                        span: Span {
                            start: 0,
                            end: delta_str.len(),
                        },
                        stage: ParseStage::Cost,
                        message: format!("invalid loyalty delta `{delta_str}`"),
                        text: line.to_string(),
                    }),
                }
                continue;
            }
            match (delta, parse_effect_body_str(body_str, vocab)) {
                (Some(d), Ok(body)) => out.abilities.push(Ability::Loyalty { delta: d, body }),
                (None, _) => out.errors.push(ParseError {
                    span: Span {
                        start: 0,
                        end: delta_str.len(),
                    },
                    stage: ParseStage::Cost,
                    message: format!("invalid loyalty delta `{delta_str}`"),
                    text: line.to_string(),
                }),
                (_, Err(errs)) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 5. Activated ability (`<cost>: <body>`, including inline modal `Choose one —` + `•` lines,
        //    and activated abilities on Instants/Sorceries such as Forecast and discard-for-Treasure!)
        if let Some((cost_str, body_str)) = split_cost_and_effect(line) {
            let cost_res = tokenize(cost_str).and_then(|toks| {
                let mut ts = TokenStream::new(cost_str, &toks, vocab, ParseStage::Cost);
                let c = parse_cost_list(&mut ts)?;
                ts.expect_eof()?;
                Ok(c)
            });
            if !is_spell || cost_res.is_ok() {
                let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
                if let Some(count) = parse_modal_header(body_str, next_is_bullet) {
                    let mut modes = Vec::new();
                    i += 1;
                    while i < lines.len() && lines[i].starts_with(BULLET) {
                        let raw_mode = lines[i].trim_start_matches(BULLET).trim();
                        let mode_text = strip_ability_word_prefix(raw_mode);
                        match parse_effect_body_str(&mode_text, vocab) {
                            Ok(body) => modes.push(ModalMode { body }),
                            Err(errs) => out.errors.extend(errs),
                        }
                        i += 1;
                    }
                    match cost_res {
                        Ok(cost) if !modes.is_empty() => out.abilities.push(Ability::Activated {
                            cost,
                            body: vec![Effect::PromptChoice {
                                chooser: PlayerRef::You,
                                kind: ChoiceKind::Modal { count, modes },
                            }],
                            restrictions: Vec::new(),
                        }),
                        Ok(_) => out.errors.push(ParseError {
                            span: Span {
                                start: 0,
                                end: line.len(),
                            },
                            stage: ParseStage::Effect,
                            message: "modal activated ability with zero modes".to_string(),
                            text: line.to_string(),
                        }),
                        Err(e) => out.errors.push(e),
                    }
                    continue;
                }
                match (cost_res, parse_effect_body_str(body_str, vocab)) {
                    (Ok(cost), Ok(mut body)) => {
                        let mut restrictions = Vec::new();
                        body.retain(|eff| match eff {
                            Effect::Restriction(rs) => {
                                restrictions.extend(rs.iter().cloned());
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
        }

        // 6. Triggered ability (`When/Whenever/At the beginning of ..., <body>`, including inline modal!)
        if !is_spell && is_trigger_line(line) {
            if let Some((header_part, modal_count)) = split_triggered_modal_header(line) {
                let mut modes = Vec::new();
                i += 1;
                while i < lines.len() && lines[i].starts_with(BULLET) {
                    let raw_mode = lines[i].trim_start_matches(BULLET).trim();
                    let mode_text = strip_ability_word_prefix(raw_mode);
                    match parse_effect_body_str(&mode_text, vocab) {
                        Ok(body) => modes.push(ModalMode { body }),
                        Err(errs) => out.errors.extend(errs),
                    }
                    i += 1;
                }
                match tokenize(header_part).and_then(|toks| {
                    let mut ts = TokenStream::new(header_part, &toks, vocab, ParseStage::Trigger);
                    let trig = parse_trigger_header(&mut ts)?;
                    let _ = ts.eat_comma();
                    ts.expect_eof()?;
                    Ok(trig)
                }) {
                    Ok(trigger) if !modes.is_empty() => out.abilities.push(Ability::Triggered {
                        trigger,
                        body: vec![Effect::PromptChoice {
                            chooser: PlayerRef::You,
                            kind: ChoiceKind::Modal {
                                count: modal_count,
                                modes,
                            },
                        }],
                    }),
                    Ok(_) => out.errors.push(ParseError {
                        span: Span {
                            start: 0,
                            end: line.len(),
                        },
                        stage: ParseStage::Effect,
                        message: "modal trigger with zero modes".to_string(),
                        text: line.to_string(),
                    }),
                    Err(e) => out.errors.push(e),
                }
                continue;
            }
            match parse_triggered_line(line, vocab) {
                Ok(ab) => out.abilities.push(ab),
                Err(errs) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 7. Standalone keyword line (`Flying, vigilance`, `Kicker—Sacrifice an artifact.`, `Equip {3}. This ability costs {1} less to activate ...`)
        if let Ok(toks) = tokenize(line) {
            let mut ts = TokenStream::new(line, &toks, vocab, ParseStage::Face);
            if let Ok((kws, _)) = parse_keyword_list(&mut ts) {
                let ate_period = ts.eat_period();
                if ts.is_eof() {
                    out.keywords.extend(kws);
                    i += 1;
                    continue;
                }
                if ate_period {
                    let mut tail_ts =
                        TokenStream::new(line, &toks[ts.pos..], vocab, ParseStage::Static);
                    if let Ok(st) = parse_static_ability(&mut tail_ts) {
                        out.keywords.extend(kws);
                        out.statics.push(st);
                        i += 1;
                        continue;
                    }
                    let mut eff_ts =
                        TokenStream::new(line, &toks[ts.pos..], vocab, ParseStage::Effect);
                    if let Ok(effs) = crate::v2::clause::parse_effect_sentence(&mut eff_ts)
                        && {
                            let _ = eff_ts.eat_period();
                            eff_ts.is_eof()
                        }
                    {
                        out.keywords.extend(kws);
                        out.spell_effects.extend(effs);
                        i += 1;
                        continue;
                    }
                }
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

        // 9. Replacement or Static ability on permanent/emblem (supports multi-sentence paragraphs
        //    such as `You may exert ~ as it attacks. When you do, ...` or `If damage would be dealt to ~, prevent that damage. Remove a +1/+1 counter from ~.`)
        match tokenize(line) {
            Ok(toks) => {
                let mut rep_ts = TokenStream::new(line, &toks, vocab, ParseStage::Replacement);
                if let Ok(rep) = parse_replacement(&mut rep_ts) {
                    out.replacements.push(rep);
                    i += 1;
                    continue;
                }
                let mut st_ts = TokenStream::new(line, &toks, vocab, ParseStage::Static);
                if let Ok(st) = parse_static_ability(&mut st_ts) {
                    out.statics.push(st);
                    i += 1;
                    continue;
                }
                // Multi-sentence permanent paragraph fallback: split on top-level `.` (outside quotes)
                // when sentence 1 is a valid Replacement or Static and remaining sentences are valid Effects/Statics.
                if let Some(first_dot) = toks
                    .iter()
                    .position(|t| matches!(t.kind, TokenKind::Period))
                    && first_dot + 1 < toks.len()
                {
                    let head_toks = &toks[..first_dot];
                    let tail_toks = &toks[first_dot + 1..];
                    let head_rep = {
                        let mut rts =
                            TokenStream::new(line, head_toks, vocab, ParseStage::Replacement);
                        parse_replacement(&mut rts).ok()
                    };
                    let head_st = if head_rep.is_none() {
                        let mut sts = TokenStream::new(line, head_toks, vocab, ParseStage::Static);
                        parse_static_ability(&mut sts).ok()
                    } else {
                        None
                    };
                    if head_rep.is_some() || head_st.is_some() {
                        let mut tail_effects = Vec::new();
                        let mut tail_statics = Vec::new();
                        let mut tail_reps = Vec::new();
                        let mut tail_ok = true;
                        let mut cur_start = 0usize;
                        for (idx, tok) in tail_toks.iter().enumerate() {
                            if matches!(tok.kind, TokenKind::Period) {
                                if idx > cur_start {
                                    let sl = &tail_toks[cur_start..idx];
                                    let mut sub =
                                        TokenStream::new(line, sl, vocab, ParseStage::Effect);
                                    if let Ok(effs) = parse_effect_sentence(&mut sub) {
                                        tail_effects.extend(effs);
                                    } else {
                                        let mut sts =
                                            TokenStream::new(line, sl, vocab, ParseStage::Static);
                                        if let Ok(st) = parse_static_ability(&mut sts) {
                                            tail_statics.push(st);
                                        } else {
                                            let mut rts = TokenStream::new(
                                                line,
                                                sl,
                                                vocab,
                                                ParseStage::Replacement,
                                            );
                                            if let Ok(rp) = parse_replacement(&mut rts) {
                                                tail_reps.push(rp);
                                            } else {
                                                tail_ok = false;
                                                break;
                                            }
                                        }
                                    }
                                }
                                cur_start = idx + 1;
                            }
                        }
                        if tail_ok && cur_start < tail_toks.len() {
                            let sl = &tail_toks[cur_start..];
                            let mut sub = TokenStream::new(line, sl, vocab, ParseStage::Effect);
                            if let Ok(effs) = parse_effect_sentence(&mut sub) {
                                tail_effects.extend(effs);
                            } else {
                                let mut sts = TokenStream::new(line, sl, vocab, ParseStage::Static);
                                if let Ok(st) = parse_static_ability(&mut sts) {
                                    tail_statics.push(st);
                                } else {
                                    let mut rts =
                                        TokenStream::new(line, sl, vocab, ParseStage::Replacement);
                                    if let Ok(rp) = parse_replacement(&mut rts) {
                                        tail_reps.push(rp);
                                    } else {
                                        tail_ok = false;
                                    }
                                }
                            }
                        }
                        if tail_ok {
                            if let Some(rep) = head_rep {
                                out.replacements.push(rep);
                            } else if let Some(st) = head_st {
                                out.statics.push(st);
                            }
                            out.spell_effects.extend(tail_effects);
                            out.statics.extend(tail_statics);
                            out.replacements.extend(tail_reps);
                            i += 1;
                            continue;
                        }
                    }
                }
                let mut err_ts = TokenStream::new(line, &toks, vocab, ParseStage::Static);
                if let Err(e) = parse_static_ability(&mut err_ts) {
                    out.errors.push(e);
                }
            }
            Err(e) => out.errors.push(e),
        }
        i += 1;
    }

    out
}

fn split_triggered_modal_header(line: &str) -> Option<(&str, ChoiceCount)> {
    let trimmed = line.trim();
    if !trimmed.ends_with(EM_DASH) {
        return None;
    }
    let idx = trimmed.rfind(", choose ")?;
    let modal_part = &trimmed[idx + 2..];
    let count = parse_modal_header(modal_part, false)?;
    Some((&trimmed[..idx], count))
}

fn parse_modal_header(line: &str, allow_no_dash: bool) -> Option<ChoiceCount> {
    let low = line.trim().to_ascii_lowercase();
    let head = if let Some(h) = low.strip_suffix(EM_DASH) {
        h.trim()
    } else if allow_no_dash {
        low.trim_end_matches('.').trim()
    } else {
        return None;
    };
    // Strip optional `that hasn't been chosen` / conditional `. if ..., choose ... instead.` suffix
    let core = head
        .split_once('.')
        .map_or(head, |(first, _)| first.trim())
        .trim_end_matches(" that hasn't been chosen")
        .trim_end_matches(" that hasn't been chosen this turn")
        .trim_end_matches(" that hasn't been chosen this game")
        .trim_end_matches(" at random")
        .trim();
    match core {
        "choose one" => Some(ChoiceCount::Exact(1)),
        "choose two" => Some(ChoiceCount::Exact(2)),
        "choose three" => Some(ChoiceCount::Exact(3)),
        "choose one or both" => Some(ChoiceCount::OneOrBoth),
        "choose one or more" => Some(ChoiceCount::OneOrMore),
        "choose up to one" => Some(ChoiceCount::UpTo(1)),
        "choose up to two" => Some(ChoiceCount::UpTo(2)),
        "choose up to three" => Some(ChoiceCount::UpTo(3)),
        "choose up to four" => Some(ChoiceCount::UpTo(4)),
        "choose four" => Some(ChoiceCount::Exact(4)),
        "choose up to x" | "choose x" => Some(ChoiceCount::VariableX),
        "choose any number" => Some(ChoiceCount::AnyNumber),
        _ => None,
    }
}

/// Strip CR 207.2c flavor-only ability word prefix (`Landfall — ...`, `Threshold — ...`,
/// `Channel — ...`, `Heroic — ...`, etc.) while preserving modal, saga, spree, case, and sticker lines.
fn strip_ability_word_prefix(line: &str) -> String {
    let Some((prefix, rest)) = line.split_once(" \u{2014} ") else {
        return line.to_string();
    };
    let p = prefix.trim();
    if p.is_empty() || p.len() > 50 || p.contains('{') || p.contains('+') {
        return line.to_string();
    }
    let low = p.to_ascii_lowercase();
    // Preserve structural em-dash headers (modal, saga, case, villainous choice)
    if low.starts_with("choose ")
        || low.contains("villainous choice")
        || matches!(
            low.as_str(),
            "i" | "ii" | "iii" | "iv" | "v" | "vi" | "to solve" | "solved"
        )
        || low.contains("i, ")
    {
        return line.to_string();
    }
    // Only strip recognized CR 207.2c ability words (all words alphabetic/hyphen/digit/quote/bang, <= 7 words)
    let words: Vec<&str> = p.split_whitespace().collect();
    if words.len() <= 7
        && words.iter().all(|w| {
            w.chars().all(|c| {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '\'' | '!' | ',' | '?' | '.')
            })
        })
        && words
            .first()
            .and_then(|w| w.chars().next())
            .is_some_and(|c| c.is_ascii_uppercase())
        && !low.starts_with("when ")
        && !low.starts_with("whenever ")
        && !low.starts_with("if ")
        && !low.starts_with("as ")
    {
        return rest.trim().to_string();
    }
    line.to_string()
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

fn split_sticker_row(line: &str) -> Option<(u8, &str)> {
    let (lhs, rhs) = line.split_once(" \u{2014} ")?;
    let mut rest = lhs.trim();
    let mut count = 0u8;
    while let Some(after) = rest.strip_prefix("{TK}") {
        count = count.saturating_add(1);
        rest = after;
    }
    if count > 0 && rest.is_empty() {
        Some((count, rhs.trim()))
    } else {
        None
    }
}

fn parse_bare_base_pt(s: &str) -> Option<(i32, i32)> {
    let (p, t) = s.trim().split_once('/')?;
    Some((p.parse().ok()?, t.parse().ok()?))
}

fn split_pipe_table_row(line: &str) -> Option<(u16, Option<u16>, bool, &str)> {
    let (lhs, rhs) = line.split_once(" | ")?;
    let h = lhs.trim();
    if let Some(num) = h.strip_suffix('+') {
        let min = num.trim().parse::<u16>().ok()?;
        return Some((min, None, true, rhs.trim()));
    }
    for sep in ["\u{2014}", "\u{2013}", "-"] {
        if let Some((a, b)) = h.split_once(sep) {
            let min = a.trim().parse::<u16>().ok()?;
            let max = b.trim().parse::<u16>().ok()?;
            return Some((min, Some(max), false, rhs.trim()));
        }
    }
    let min = h.parse::<u16>().ok()?;
    Some((min, Some(min), false, rhs.trim()))
}

fn is_trigger_line(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    low.starts_with("when ")
        || low.starts_with("whenever ")
        || low.starts_with("at the beginning of ")
        || low.starts_with("at the end of ")
        || low.starts_with("at end of combat")
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
        let is_period = matches!(toks[idx].kind, TokenKind::Period);
        let is_sentence_ending_quote = matches!(toks[idx].kind, TokenKind::Quoted(q) if q.trim_end().ends_with('.'))
            && toks.get(idx + 1).is_some_and(|next| {
                matches!(
                    next.kind,
                    TokenKind::Word(w)
                        if matches!(
                            w,
                            "It's"
                                | "They're"
                                | "Activate"
                                | "Do"
                                | "This"
                                | "Exile"
                                | "Sacrifice"
                                | "Return"
                                | "Draw"
                                | "Put"
                                | "You"
                                | "Each"
                                | "If"
                                | "When"
                                | "Whenever"
                                | "At"
                                | "Until"
                                | "Then"
                                | "Otherwise"
                                | "The"
                        )
                )
            });
        if is_period || is_sentence_ending_quote {
            let end_idx = if is_period { idx } else { idx + 1 };
            if end_idx > cur_start {
                let mut sub =
                    TokenStream::new(line, &toks[cur_start..end_idx], vocab, ParseStage::Effect);
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
