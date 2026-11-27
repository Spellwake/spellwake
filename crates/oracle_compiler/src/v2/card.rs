//! Top-level card and face compiler for `v2`.
//!
//! Strips multi-line reminder text, normalizes card self-references to `~`, dispatches each
//! paragraph by syntactic structure (Saga chapters, Planeswalker loyalty, Modal/Spree blocks,
//! Activated abilities, Triggered abilities, Keyword lines, Replacements, Statics, Spell effects),
//! and records every unparsed clause as an explicit fail-closed [`ParseError`].

use crate::ast::{CardTypeMask, ColorMask};
use crate::preprocess::{
    BULLET, EM_DASH, normalize_self, split_cost_and_effect, split_loyalty_line, split_saga_line,
    strip_reminder, strip_reminder_multiline,
};
use crate::v2::clause::{parse_effect_body_str, parse_effect_sentence};
use crate::v2::cost::parse_cost_list;
use crate::v2::filter::{parse_keyword_list, parse_subject_ref};
use crate::v2::ir::{
    Ability, ChoiceCount, ChoiceKind, CompiledCardV2, CompiledFaceV2, Cost, DelayedTriggerKind,
    Duration, Effect, EntryModifiers, LoyaltyDelta, ManaCost, ManaSymbol, ModalMode,
    ObjectMutation, ParseError, ParseStage, PlayerRef, Replacement, Span, SpreeMode,
    StickerPayload, SubjectRef, TableRowPayload, TokenSpec, TriggerSpec, ValueExpr,
};
use crate::v2::lexer::{TokenKind, tokenize};
use crate::v2::parser::{TokenStream, try_parse};
use crate::v2::replacement::parse_replacement;
use crate::v2::static_ability::parse_static_ability;
use crate::v2::trigger::parse_trigger_header;
use crate::vocab::lookup_color;
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

    let normalized_quotes = face
        .oracle_text
        .replace(['\u{201C}', '\u{201D}'], "\"")
        .replace(['\u{2018}', '\u{2019}'], "'");
    let stripped_full = strip_reminder_multiline(&normalized_quotes);
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

        // 0a. Sticker row (`{TK}{TK} - 1/5` or `{TK}{TK}{TK} - Whenever ~ attacks, ...`)
        if let Some((ticket_cost, rhs)) = split_repeated_symbol_row(line, "{TK}") {
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

        // 0b. Die-roll table / Station threshold row (`1-9 | ...`, `20 | ...`, `9+ | Flying`)
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

        // 0c. Case enchantment rows (`To solve - <cond>` / `Solved - <ability>`)
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

        // 1. Pawprint modal block (`Choose up to five {P} worth of modes...` followed by `{P} - ...` lines)
        let next_is_pawprint = lines
            .get(i + 1)
            .is_some_and(|l| split_repeated_symbol_row(l, "{P}").is_some());
        if next_is_pawprint && low_line.starts_with("choose ") && low_line.contains("{p}") {
            let mut paw_modes = Vec::new();
            i += 1;
            while i < lines.len()
                && let Some((cost_n, body_raw)) = split_repeated_symbol_row(&lines[i], "{P}")
            {
                let cost = vec![Cost::Mana(ManaCost {
                    symbols: vec![ManaSymbol::Pawprint; usize::from(cost_n)],
                })];
                match parse_modal_mode_body(body_raw, vocab) {
                    Ok(body) => paw_modes.push(SpreeMode { cost, body }),
                    Err(errs) => out.errors.extend(errs),
                }
                i += 1;
            }
            push_spree_choice(&mut out, paw_modes);
            continue;
        }

        // 1b. Modal block (`Choose one -`, `An opponent chooses one -`, `Choose five. ...` followed by bullet lines)
        let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
        if let Some(hdr) = parse_modal_header(line, next_is_bullet) {
            i += 1;
            let modes = collect_modal_modes(&lines, &mut i, vocab, &mut out.errors);
            if modes.is_empty() {
                out.errors.push(make_parse_error(
                    line.len(),
                    ParseStage::Effect,
                    "modal header with zero modes",
                    line,
                ));
            } else {
                out.spell_effects
                    .extend(build_modal_effects(hdr, modes, vocab, &mut out.errors));
            }
            continue;
        }

        // 2. Spree block (`Spree` followed by `+ {cost} - <body>` lines)
        if line.eq_ignore_ascii_case("spree") {
            push_header_keywords(line, vocab, &mut out);
            let mut spree_modes = Vec::new();
            i += 1;
            while i < lines.len() && lines[i].starts_with("+ ") {
                if let Some((cost_raw, body_raw)) = lines[i]["+ ".len()..].split_once(EM_DASH) {
                    match (
                        parse_cost_str(cost_raw.trim(), vocab),
                        parse_modal_mode_body(body_raw.trim(), vocab),
                    ) {
                        (Ok(cost), Ok(body)) => spree_modes.push(SpreeMode { cost, body }),
                        (Err(e), _) => out.errors.push(e),
                        (_, Err(errs)) => out.errors.extend(errs),
                    }
                } else {
                    out.errors.push(make_parse_error(
                        lines[i].len(),
                        ParseStage::Effect,
                        "malformed Spree mode line",
                        &lines[i],
                    ));
                }
                i += 1;
            }
            push_spree_choice(&mut out, spree_modes);
            continue;
        }

        // 2b. Tiered block (`Tiered` followed by bullet `<Label> - <Cost> - <Body>` lines)
        if line.eq_ignore_ascii_case("tiered") {
            push_header_keywords(line, vocab, &mut out);
            i += 1;
            let tiered_modes = collect_tiered_modes(&lines, &mut i, vocab, &mut out.errors);
            push_spree_choice(&mut out, tiered_modes);
            continue;
        }

        // 3. Saga chapter line (`I - ...`, `II - ...`, `III - ...`, including inline modal)
        if is_saga && let Some((chapters_str, body_str)) = split_saga_line(line) {
            let chapters: Vec<u8> = chapters_str.iter().filter_map(|r| roman_to_u8(r)).collect();
            let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
            if let Some(hdr) = parse_modal_header(body_str, next_is_bullet) {
                i += 1;
                let modes = collect_modal_modes(&lines, &mut i, vocab, &mut out.errors);
                if !modes.is_empty() {
                    let body = build_modal_effects(hdr, modes, vocab, &mut out.errors);
                    out.abilities.push(Ability::SagaChapter { chapters, body });
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

        // 4. Planeswalker loyalty line (`+1: ...`, `-2: ...`, `-7: ...`, including inline modals)
        if !is_spell && let Some((delta_str, body_str)) = split_loyalty_line(line) {
            let delta = parse_loyalty_delta(&delta_str);
            let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
            if let Some(hdr) = parse_modal_header(body_str, next_is_bullet) {
                i += 1;
                let modes = collect_modal_modes(&lines, &mut i, vocab, &mut out.errors);
                match delta {
                    Some(d) if !modes.is_empty() => {
                        let body = build_modal_effects(hdr, modes, vocab, &mut out.errors);
                        out.abilities.push(Ability::Loyalty { delta: d, body });
                    }
                    Some(_) => out.errors.push(make_parse_error(
                        line.len(),
                        ParseStage::Effect,
                        "modal loyalty ability with zero modes",
                        line,
                    )),
                    None => out.errors.push(make_parse_error(
                        delta_str.len(),
                        ParseStage::Cost,
                        format!("invalid loyalty delta `{delta_str}`"),
                        line,
                    )),
                }
                continue;
            }
            match (delta, parse_effect_body_str(body_str, vocab)) {
                (Some(d), Ok(body)) => out.abilities.push(Ability::Loyalty { delta: d, body }),
                (None, _) => out.errors.push(make_parse_error(
                    delta_str.len(),
                    ParseStage::Cost,
                    format!("invalid loyalty delta `{delta_str}`"),
                    line,
                )),
                (_, Err(errs)) => out.errors.extend(errs),
            }
            i += 1;
            continue;
        }

        // 5. Activated ability (`<cost>: <body>`, including inline modal and spell-zone activations)
        if let Some((cost_str, body_str)) = split_cost_and_effect(line) {
            let cost_res = parse_cost_str(cost_str, vocab);
            if !is_spell || cost_res.is_ok() {
                let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
                if let Some(hdr) = parse_modal_header(body_str, next_is_bullet) {
                    i += 1;
                    let modes = collect_modal_modes(&lines, &mut i, vocab, &mut out.errors);
                    match cost_res {
                        Ok(cost) if !modes.is_empty() => {
                            let body = build_modal_effects(hdr, modes, vocab, &mut out.errors);
                            out.abilities.push(Ability::Activated {
                                cost,
                                body,
                                restrictions: Vec::new(),
                            });
                        }
                        Ok(_) => out.errors.push(make_parse_error(
                            line.len(),
                            ParseStage::Effect,
                            "modal activated ability with zero modes",
                            line,
                        )),
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

        // 6. Triggered ability (`When/Whenever/At ..., <body>`, including inline modals, reflexive modals, and Haunt triggers on spells)
        if is_trigger_line(line) {
            if is_spell {
                if let Ok(ab) = parse_triggered_line(line, vocab) {
                    out.abilities.push(ab);
                    i += 1;
                    continue;
                }
            } else {
                let next_is_bullet = lines.get(i + 1).is_some_and(|l| l.starts_with(BULLET));
                if next_is_bullet
                    && try_compile_triggered_modal(line, &lines, &mut i, vocab, &mut out)
                {
                    continue;
                }
                match parse_triggered_line(line, vocab) {
                    Ok(ab) => out.abilities.push(ab),
                    Err(errs) => out.errors.extend(errs),
                }
                i += 1;
                continue;
            }
        }

        // 6b. Standalone bullet lines (Tiered modes, die-roll keyword bullets, Siege / named-choice bullets)
        if line.starts_with(BULLET) {
            if split_tiered_bullet(line).is_some() {
                let tiered_modes = collect_tiered_modes(&lines, &mut i, vocab, &mut out.errors);
                push_spree_choice(&mut out, tiered_modes);
                continue;
            }
            let raw_bullet = line.trim_start_matches(BULLET).trim();
            if let Some((num_str, rhs)) = raw_bullet.split_once(" \u{2014} ")
                && let Ok(min) = num_str.trim().parse::<u16>()
            {
                let compiled = compile_face_v2(
                    FaceInput {
                        name: face.name,
                        type_line: face.type_line,
                        oracle_text: rhs.trim(),
                    },
                    vocab,
                );
                if compiled.is_ok() {
                    out.abilities.push(Ability::TableRow {
                        min,
                        max: Some(min),
                        open_ended: false,
                        payload: TableRowPayload::Face(Box::new(compiled)),
                    });
                } else {
                    out.errors.extend(compiled.errors);
                }
                i += 1;
                continue;
            }
            let bullet_body = strip_bullet_label(raw_bullet);
            let compiled = compile_face_v2(
                FaceInput {
                    name: face.name,
                    type_line: face.type_line,
                    oracle_text: &bullet_body,
                },
                vocab,
            );
            if compiled.is_ok() {
                out.keywords.extend(compiled.keywords);
                out.abilities.extend(compiled.abilities);
                out.spell_effects.extend(compiled.spell_effects);
                out.statics.extend(compiled.statics);
                out.replacements.extend(compiled.replacements);
            } else {
                out.errors.extend(compiled.errors);
            }
            i += 1;
            continue;
        }

        // 7. Standalone keyword line (`Flying, vigilance`, `Kicker - Sacrifice an artifact.`, `Equip {3}. This ability costs {1} less to activate ...`)
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
                    let mut rep_ts =
                        TokenStream::new(line, &toks[ts.pos..], vocab, ParseStage::Replacement);
                    if let Ok(rep) = parse_replacement(&mut rep_ts) {
                        out.keywords.extend(kws);
                        out.replacements.push(rep);
                        i += 1;
                        continue;
                    }
                    if is_spell {
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
                // when sentence 1 is a valid Replacement or Static. Follow-up effect sentences are
                // ONLY attached to `EventInstead` / `AsEnters` replacements (never detached into
                // `out.spell_effects` on non-spell cards).
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
                        let can_attach_effects = matches!(
                            head_rep,
                            Some(Replacement::EventInstead { .. } | Replacement::AsEnters(_))
                        );
                        let mut tail_effects = Vec::new();
                        let mut tail_statics = Vec::new();
                        let mut tail_reps = Vec::new();
                        let mut parse_tail_slice = |sl: &[crate::v2::lexer::Token<'_>]| -> bool {
                            let mut sts = TokenStream::new(line, sl, vocab, ParseStage::Static);
                            if let Ok(st) = parse_static_ability(&mut sts) {
                                tail_statics.push(st);
                                return true;
                            }
                            let mut rts =
                                TokenStream::new(line, sl, vocab, ParseStage::Replacement);
                            if let Ok(rp) = parse_replacement(&mut rts) {
                                tail_reps.push(rp);
                                return true;
                            }
                            if can_attach_effects {
                                let mut sub = TokenStream::new(line, sl, vocab, ParseStage::Effect);
                                if let Ok(effs) = parse_effect_sentence(&mut sub) {
                                    tail_effects.extend(effs);
                                    return true;
                                }
                            }
                            false
                        };
                        let mut tail_ok = true;
                        let mut cur_start = 0usize;
                        for (idx, tok) in tail_toks.iter().enumerate() {
                            if matches!(tok.kind, TokenKind::Period) {
                                if idx > cur_start && !parse_tail_slice(&tail_toks[cur_start..idx])
                                {
                                    tail_ok = false;
                                    break;
                                }
                                cur_start = idx + 1;
                            }
                        }
                        if tail_ok && cur_start < tail_toks.len() {
                            tail_ok = parse_tail_slice(&tail_toks[cur_start..]);
                        }
                        if tail_ok {
                            if let Some(mut rep) = head_rep {
                                if !tail_effects.is_empty()
                                    && let Replacement::EventInstead { instead, .. }
                                    | Replacement::AsEnters(instead) = &mut rep
                                {
                                    instead.extend(tail_effects);
                                }
                                out.replacements.push(rep);
                            } else if let Some(st) = head_st {
                                out.statics.push(st);
                            }
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

struct ModalHeaderInfo {
    chooser: PlayerRef,
    count: ChoiceCount,
    optional: bool,
    extra_effects: Option<String>,
}

const MODAL_CHOOSER_PREFIXES: &[(&str, PlayerRef, bool)] = &[
    ("an opponent chooses ", PlayerRef::AnyOpponent, false),
    ("target opponent chooses ", PlayerRef::TargetOpponent, false),
    ("target player chooses ", PlayerRef::TargetPlayer, false),
    (
        "defending player chooses ",
        PlayerRef::DefendingPlayer,
        false,
    ),
    ("each player chooses ", PlayerRef::EachPlayer, false),
    ("each opponent chooses ", PlayerRef::EachOpponent, false),
    ("that player chooses ", PlayerRef::ThatPlayer, false),
    ("you may choose ", PlayerRef::You, true),
    ("choose ", PlayerRef::You, false),
];

const MODAL_COUNT_TABLE: &[(&str, ChoiceCount)] = &[
    ("one", ChoiceCount::Exact(1)),
    ("two", ChoiceCount::Exact(2)),
    ("three", ChoiceCount::Exact(3)),
    ("four", ChoiceCount::Exact(4)),
    ("five", ChoiceCount::Exact(5)),
    ("one or both", ChoiceCount::OneOrBoth),
    ("one or more", ChoiceCount::OneOrMore),
    ("up to one", ChoiceCount::UpTo(1)),
    ("up to two", ChoiceCount::UpTo(2)),
    ("up to three", ChoiceCount::UpTo(3)),
    ("up to four", ChoiceCount::UpTo(4)),
    ("up to five", ChoiceCount::UpTo(5)),
    ("up to x", ChoiceCount::VariableX),
    ("x", ChoiceCount::VariableX),
    ("up to that many", ChoiceCount::VariableX),
    ("that many", ChoiceCount::VariableX),
    ("any number", ChoiceCount::AnyNumber),
];

fn parse_modal_header(line: &str, allow_no_dash: bool) -> Option<ModalHeaderInfo> {
    let trimmed = line.trim();
    let head = if let Some(h) = trimmed.strip_suffix(EM_DASH) {
        h.trim()
    } else if allow_no_dash {
        trimmed.trim_end_matches('.').trim()
    } else {
        return None;
    };
    if head.contains('.') {
        return None;
    }
    let first_low = head.to_ascii_lowercase();
    let (chooser, optional, prefix_len) = MODAL_CHOOSER_PREFIXES
        .iter()
        .find_map(|&(pfx, p, opt)| first_low.starts_with(pfx).then_some((p, opt, pfx.len())))?;
    let rest_raw = head[prefix_len..].trim();
    let rest_low = rest_raw.to_ascii_lowercase();
    let (core, extra_effects) = if let Some(and_idx) = rest_low.find(" and ") {
        let after = rest_raw[and_idx + 5..].trim();
        let extra = (!after.is_empty()).then(|| after.to_string());
        (rest_low[..and_idx].trim(), extra)
    } else {
        (rest_low.as_str(), None)
    };
    let count = MODAL_COUNT_TABLE
        .iter()
        .find_map(|(k, c)| (*k == core).then_some(*c))?;
    Some(ModalHeaderInfo {
        chooser,
        count,
        optional,
        extra_effects,
    })
}

enum ReflexiveModalHeader {
    WhenYouDo,
    DelayedEvent(TriggerSpec),
}

fn split_reflexive_modal<'a>(
    rest: &'a str,
    vocab: &Vocabulary,
) -> Option<(&'a str, ReflexiveModalHeader, ModalHeaderInfo)> {
    let low = rest.to_ascii_lowercase();
    let dot_idx = low.rfind(". when ")?;
    let pre_str = rest[..dot_idx].trim();
    if pre_str.is_empty() {
        return None;
    }
    let reflex_str = rest[dot_idx + 2..].trim();
    for (idx, _) in reflex_str.rmatch_indices(", ") {
        let header_part = reflex_str[..idx].trim();
        let modal_part = reflex_str[idx + 2..].trim();
        let Some(hdr) = parse_modal_header(modal_part, true) else {
            continue;
        };
        if header_part.eq_ignore_ascii_case("when you do") {
            return Some((pre_str, ReflexiveModalHeader::WhenYouDo, hdr));
        }
        if let Ok(htoks) = tokenize(header_part) {
            let mut hts = TokenStream::new(header_part, &htoks, vocab, ParseStage::Trigger);
            if let Ok(trig) = parse_trigger_header(&mut hts)
                && hts.is_eof()
            {
                return Some((pre_str, ReflexiveModalHeader::DelayedEvent(trig), hdr));
            }
        }
    }
    None
}

fn try_compile_triggered_modal(
    line: &str,
    lines: &[String],
    i: &mut usize,
    vocab: &Vocabulary,
    out: &mut CompiledFaceV2,
) -> bool {
    let Ok(toks) = tokenize(line) else {
        return false;
    };
    let mut ts = TokenStream::new(line, &toks, vocab, ParseStage::Trigger);
    let Ok(trigger) = parse_trigger_header(&mut ts) else {
        return false;
    };
    if !ts.eat_comma() {
        return false;
    }
    let rest = ts
        .peek()
        .map_or("", |t| line.get(t.span.start..).unwrap_or(""))
        .trim();

    if let Some(hdr) = parse_modal_header(rest, true) {
        *i += 1;
        let modes = collect_modal_modes(lines, i, vocab, &mut out.errors);
        if modes.is_empty() {
            out.errors.push(make_parse_error(
                line.len(),
                ParseStage::Effect,
                "modal trigger with zero modes",
                line,
            ));
        } else {
            let body = build_modal_effects(hdr, modes, vocab, &mut out.errors);
            out.abilities.push(Ability::Triggered { trigger, body });
        }
        return true;
    }

    if let Some((pre_str, reflex_hdr, hdr)) = split_reflexive_modal(rest, vocab) {
        *i += 1;
        let modes = collect_modal_modes(lines, i, vocab, &mut out.errors);
        match parse_effect_body_str(pre_str, vocab) {
            Ok(mut body) if !modes.is_empty() => {
                let reflex_body = build_modal_effects(hdr, modes, vocab, &mut out.errors);
                match reflex_hdr {
                    ReflexiveModalHeader::WhenYouDo => {
                        body.push(Effect::ReflexiveTrigger(reflex_body));
                    }
                    ReflexiveModalHeader::DelayedEvent(delayed_trig) => {
                        body.push(Effect::DelayedTrigger {
                            trigger: DelayedTriggerKind::Event(delayed_trig),
                            body: reflex_body,
                        });
                    }
                }
                out.abilities.push(Ability::Triggered { trigger, body });
            }
            Ok(_) => out.errors.push(make_parse_error(
                line.len(),
                ParseStage::Effect,
                "modal trigger with zero modes",
                line,
            )),
            Err(errs) => out.errors.extend(errs),
        }
        return true;
    }

    false
}

fn make_parse_error(
    end: usize,
    stage: ParseStage,
    message: impl Into<String>,
    text: &str,
) -> ParseError {
    ParseError {
        span: Span { start: 0, end },
        stage,
        message: message.into(),
        text: text.to_string(),
    }
}

fn parse_cost_str(cost_raw: &str, vocab: &Vocabulary) -> Result<Vec<Cost>, ParseError> {
    tokenize(cost_raw).and_then(|toks| {
        let mut ts = TokenStream::new(cost_raw, &toks, vocab, ParseStage::Cost);
        let c = parse_cost_list(&mut ts)?;
        ts.expect_eof()?;
        Ok(c)
    })
}

fn build_modal_effects(
    hdr: ModalHeaderInfo,
    modes: Vec<ModalMode>,
    vocab: &Vocabulary,
    errors: &mut Vec<ParseError>,
) -> Vec<Effect> {
    let prompt = Effect::PromptChoice {
        chooser: hdr.chooser,
        kind: ChoiceKind::Modal {
            count: hdr.count,
            modes,
        },
    };
    let mut body = vec![if hdr.optional {
        Effect::Optional {
            actor: hdr.chooser,
            body: vec![prompt],
        }
    } else {
        prompt
    }];
    if let Some(extra) = hdr.extra_effects {
        match parse_effect_body_str(&extra, vocab) {
            Ok(effs) => body.extend(effs),
            Err(errs) => errors.extend(errs),
        }
    }
    body
}

fn push_spree_choice(out: &mut CompiledFaceV2, modes: Vec<SpreeMode>) {
    if !modes.is_empty() {
        out.spell_effects.push(Effect::PromptChoice {
            chooser: PlayerRef::You,
            kind: ChoiceKind::Spree { modes },
        });
    }
}

fn push_header_keywords(line: &str, vocab: &Vocabulary, out: &mut CompiledFaceV2) {
    if let Ok(toks) = tokenize(line) {
        let mut ts = TokenStream::new(line, &toks, vocab, ParseStage::Face);
        if let Ok((kws, _)) = parse_keyword_list(&mut ts) {
            out.keywords.extend(kws);
        }
    }
}

fn collect_modal_modes(
    lines: &[String],
    i: &mut usize,
    vocab: &Vocabulary,
    errors: &mut Vec<ParseError>,
) -> Vec<ModalMode> {
    let mut modes = Vec::new();
    while *i < lines.len() && lines[*i].starts_with(BULLET) {
        let raw_mode = lines[*i].trim_start_matches(BULLET).trim();
        match parse_modal_mode_body(raw_mode, vocab) {
            Ok(body) => modes.push(ModalMode { body }),
            Err(errs) => errors.extend(errs),
        }
        *i += 1;
    }
    modes
}

fn split_tiered_bullet(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix(BULLET)?.trim();
    let (_label, after_first) = rest.split_once(" \u{2014} ")?;
    let (cost_str, body_str) = after_first.split_once(" \u{2014} ")?;
    let c = cost_str.trim();
    let b = body_str.trim();
    if c.is_empty() || b.is_empty() || !c.contains('{') {
        return None;
    }
    Some((c, b))
}

fn collect_tiered_modes(
    lines: &[String],
    i: &mut usize,
    vocab: &Vocabulary,
    errors: &mut Vec<ParseError>,
) -> Vec<SpreeMode> {
    let mut modes = Vec::new();
    while *i < lines.len()
        && let Some((cost_raw, body_raw)) = split_tiered_bullet(&lines[*i])
    {
        match (
            parse_cost_str(cost_raw, vocab),
            parse_modal_mode_body(body_raw, vocab),
        ) {
            (Ok(cost), Ok(body)) => modes.push(SpreeMode { cost, body }),
            (Err(e), _) => errors.push(e),
            (_, Err(errs)) => errors.extend(errs),
        }
        *i += 1;
    }
    modes
}

fn split_repeated_symbol_row<'a>(line: &'a str, sym: &str) -> Option<(u8, &'a str)> {
    let (lhs, rhs) = line.split_once(" \u{2014} ")?;
    let mut rest = lhs.trim();
    let mut count = 0u8;
    while let Some(after) = rest.strip_prefix(sym) {
        count = count.saturating_add(1);
        rest = after;
    }
    (count > 0 && rest.is_empty()).then_some((count, rhs.trim()))
}

fn strip_bullet_label(raw: &str) -> String {
    if let Some(after) = raw.strip_prefix("~ \u{2014} ") {
        return after.trim().to_string();
    }
    strip_ability_word_prefix(raw)
}

fn parse_modal_mode_body(
    raw_mode: &str,
    vocab: &Vocabulary,
) -> Result<Vec<Effect>, Vec<ParseError>> {
    let mode_clean = strip_bullet_label(raw_mode);
    match parse_effect_body_str(&mode_clean, vocab) {
        Ok(effs) => Ok(effs),
        Err(errs) => {
            if let Some(char_effs) = parse_characteristic_bullet(&mode_clean, vocab) {
                return Ok(char_effs);
            }
            let trimmed = mode_clean.trim().trim_end_matches('.');
            if let Ok(toks) = tokenize(trimmed) {
                let mut ts = TokenStream::new(trimmed, &toks, vocab, ParseStage::Effect);
                if let Ok(subj) = try_parse(&mut ts, parse_subject_ref)
                    && ts.is_eof()
                {
                    return Ok(vec![Effect::PromptChoice {
                        chooser: PlayerRef::You,
                        kind: ChoiceKind::ChooseObject(subj),
                    }]);
                }
            }
            Err(errs)
        }
    }
}

fn parse_characteristic_bullet(mode: &str, vocab: &Vocabulary) -> Option<Vec<Effect>> {
    let trimmed = mode.trim().trim_end_matches('.');
    let toks = tokenize(trimmed).ok()?;
    let mut ts = TokenStream::new(trimmed, &toks, vocab, ParseStage::Effect);
    let (p, t) = ts.next_base_pt()?;
    if ts.is_eof() {
        return Some(vec![Effect::MutateObject {
            subject: SubjectRef::SelfObject,
            mutation: ObjectMutation::SetDynamicBasePT {
                power: ValueExpr::Const(p),
                toughness: ValueExpr::Const(t),
                add_types: CardTypeMask::EMPTY,
                keep_existing_types: true,
            },
            duration: Duration::UntilEndOfTurn,
        }]);
    }
    let mut colors = ColorMask::EMPTY;
    let mut subtypes = Vec::new();
    while let Some(w) = ts.peek_word() {
        let low = w.to_ascii_lowercase();
        if let Some(c) = lookup_color(&low) {
            ts.next_word();
            colors = colors.union(c);
            let _ = ts.eat_word("and");
        } else if let Some(sub) = vocab.lookup_subtype(&low) {
            ts.next_word();
            subtypes.push(sub.to_string());
        } else {
            break;
        }
    }
    let mut keywords = Vec::new();
    let mut abilities = Vec::new();
    if ts.eat_word("with") {
        if let Ok((kws, _)) = try_parse(&mut ts, parse_keyword_list) {
            keywords.extend(kws);
        }
        let _ = ts.eat_word("and");
        if let Some(quoted) = ts.next_quoted() {
            let compiled = compile_face_v2(
                FaceInput {
                    name: "",
                    type_line: "Creature",
                    oracle_text: quoted,
                },
                vocab,
            );
            if !compiled.is_ok() {
                return None;
            }
            abilities.push(compiled);
        }
    }
    if !ts.is_eof() || (subtypes.is_empty() && keywords.is_empty()) {
        return None;
    }
    Some(vec![Effect::CreateToken {
        controller: PlayerRef::You,
        count: ValueExpr::Const(1),
        spec: TokenSpec::Creature {
            power: Some(ValueExpr::Const(p)),
            toughness: Some(ValueExpr::Const(t)),
            colors,
            card_types: CardTypeMask::CREATURE,
            subtypes,
            keywords,
            abilities,
        },
        modifiers: EntryModifiers::default(),
    }])
}

/// Strip CR 207.2c flavor-only ability word prefix (`Landfall`, `Threshold`, `Channel`,
/// `Heroic`) while preserving modal, saga, spree, case, attraction, and sticker lines.
fn strip_ability_word_prefix(line: &str) -> String {
    let Some((prefix, rest)) = line.split_once(" \u{2014} ") else {
        return line.to_string();
    };
    let p = prefix.trim();
    if p.is_empty() || p.len() > 50 || p.contains('{') || p.contains('+') {
        return line.to_string();
    }
    let low = p.to_ascii_lowercase();
    // Preserve structural em-dash headers (modal, saga, case, attraction, villainous choice)
    if low.starts_with("choose ")
        || low.contains("villainous choice")
        || matches!(
            low.as_str(),
            "i" | "ii" | "iii" | "iv" | "v" | "vi" | "to solve" | "solved" | "visit" | "prize"
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
    let mut prev_sentence_start = 0usize;
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
                let slice = &toks[cur_start..end_idx];
                if !crate::v2::clause::try_merge_still_type_sentence(
                    &mut body[prev_sentence_start..],
                    slice,
                    line,
                    vocab,
                ) {
                    let mut sub = TokenStream::new(line, slice, vocab, ParseStage::Effect);
                    let effs = parse_effect_sentence(&mut sub).map_err(|e| vec![e])?;
                    prev_sentence_start = body.len();
                    body.extend(effs);
                }
            }
            cur_start = idx + 1;
        }
    }
    if cur_start < toks.len() {
        let slice = &toks[cur_start..];
        if !crate::v2::clause::try_merge_still_type_sentence(
            &mut body[prev_sentence_start..],
            slice,
            line,
            vocab,
        ) {
            let mut sub = TokenStream::new(line, slice, vocab, ParseStage::Effect);
            let effs = parse_effect_sentence(&mut sub).map_err(|e| vec![e])?;
            body.extend(effs);
        }
    }
    Ok(Ability::Triggered { trigger, body })
}
