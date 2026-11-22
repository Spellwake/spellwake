//! Standalone zero-unsafe Phase 4 Oracle Clause Compiler for Spellwake.
//!
//! Compiles Magic Oracle text across all 33,138 real-paper cards (Base Constructed/Commander,
//! Planechase, Archenemy, Conspiracy, and Paper Un-cards with Tabletop Honor-System primitives)
//! into strongly-typed, `serde`-serializable ASTs ready for thin lowering into `spellwake-engine`.

pub mod ast;
pub mod clauses;
pub mod filter;
pub mod keywords;
pub mod preprocess;
pub mod vocab;

use std::error::Error;
use std::fmt;

use ast::{
    AbilityAst, CardAst, FaceAst, InstructionAst, ModalModeAst, ParseFailureAst, StaticAbilityAst,
    TriggerEventAst,
};
use clauses::{
    StaticOrReplacement, parse_cost_list, parse_effect_body, parse_static_or_replacement,
    parse_trigger_header,
};
use keywords::try_parse_keyword_line;
use preprocess::{
    BULLET, is_bare_pt, is_exotic_layout_header, normalize_self, parse_leveler_band,
    parse_sticker_option, split_class_level_line, split_cost_and_effect, split_loyalty_line,
    split_saga_line, split_sentences, strip_ability_word, strip_reminder,
};
pub use vocab::Vocabulary;

/// Typed library error for vocabulary loading and I/O failures.
#[derive(Debug)]
pub enum CompilerError {
    /// Underlying filesystem error.
    Io(std::io::Error),
    /// Underlying JSON deserialization error.
    Json(serde_json::Error),
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "i/o failure reading compiler input: {err}"),
            Self::Json(err) => write!(f, "json parse failure reading compiler input: {err}"),
        }
    }
}

impl Error for CompilerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Json(err) => Some(err),
        }
    }
}

impl From<std::io::Error> for CompilerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for CompilerError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

/// Borrowed face input for compilation.
#[derive(Debug, Clone, Copy)]
pub struct FaceInput<'a> {
    pub name: &'a str,
    pub type_line: &'a str,
    pub oracle_text: &'a str,
}

/// Borrowed card input for compilation.
#[derive(Debug, Clone)]
pub struct CardInput<'a> {
    pub oracle_id: &'a str,
    pub name: &'a str,
    pub layout: &'a str,
    pub faces: Vec<FaceInput<'a>>,
}

/// Compilation options controlling opt-in Un-card tabletop honor-system fallbacks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileOptions {
    /// When `true`, physical/social/dexterity clauses compile into `ManualTabletop*` AST nodes.
    /// Must remain `false` for black-bordered Constructed/Commander cards so real parsing gaps
    /// are never masked.
    pub allow_tabletop_honor_system: bool,
}

/// Compile one card across all of its faces using strict standard options (`allow_tabletop_honor_system = false`).
#[must_use]
pub fn compile_card(input: &CardInput<'_>, vocab: &Vocabulary) -> CardAst {
    compile_card_with_options(input, vocab, CompileOptions::default())
}

/// Compile one card across all of its faces with explicit [`CompileOptions`].
#[must_use]
pub fn compile_card_with_options(
    input: &CardInput<'_>,
    vocab: &Vocabulary,
    options: CompileOptions,
) -> CardAst {
    let faces: Vec<FaceAst> = input
        .faces
        .iter()
        .map(|f| compile_face_with_options(*f, vocab, options))
        .collect();
    let strictly_compiled = faces.iter().all(|f| f.failures.is_empty());
    let uses_tabletop_honor_system =
        options.allow_tabletop_honor_system && faces.iter().any(face_uses_honor_system);
    CardAst {
        oracle_id: input.oracle_id.to_string(),
        name: input.name.to_string(),
        layout: input.layout.to_string(),
        faces,
        strictly_compiled,
        uses_tabletop_honor_system,
    }
}

/// Recursively compile a quoted granted ability string into a typed `FaceAst` fragment.
/// Returns `Some` only when the quoted ability compiles cleanly with zero failures.
#[must_use]
pub fn compile_granted_ability(ability_text: &str, vocab: &Vocabulary) -> Option<Box<FaceAst>> {
    let trimmed = ability_text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let face = compile_face_with_options(
        FaceInput {
            name: "",
            type_line: "Creature",
            oracle_text: trimmed,
        },
        vocab,
        CompileOptions {
            allow_tabletop_honor_system: false,
        },
    );
    if face.failures.is_empty()
        && (!face.keywords.is_empty()
            || !face.abilities.is_empty()
            || !face.statics.is_empty()
            || !face.replacements.is_empty())
    {
        Some(Box::new(face))
    } else {
        None
    }
}

/// Compile a single card face's Oracle text using strict standard options.
#[must_use]
pub fn compile_face(face: FaceInput<'_>, vocab: &Vocabulary) -> FaceAst {
    compile_face_with_options(face, vocab, CompileOptions::default())
}

/// Compile a single card face's Oracle text with explicit [`CompileOptions`].
#[must_use]
pub fn compile_face_with_options(
    face: FaceInput<'_>,
    vocab: &Vocabulary,
    options: CompileOptions,
) -> FaceAst {
    let mut out = FaceAst {
        name: face.name.to_string(),
        type_line: face.type_line.to_string(),
        ..FaceAst::default()
    };
    let is_spell = (face.type_line.contains("Instant") || face.type_line.contains("Sorcery"))
        && !(face.type_line.contains("Creature")
            || face.type_line.contains("Artifact")
            || face.type_line.contains("Enchantment")
            || face.type_line.contains("Land")
            || face.type_line.contains("Planeswalker"));
    let is_planeswalker = face.type_line.contains("Planeswalker");
    let is_saga = face.type_line.contains("Saga");
    let is_conspiracy = face.type_line.contains("Conspiracy");
    let is_attraction = face.type_line.contains("Attraction");

    let mut modal_in_spell = false;
    let mut modal_in_last_ability = false;

    for raw_p in face.oracle_text.split('\n') {
        if raw_p.trim().is_empty() {
            continue;
        }
        let cleaned = strip_reminder(raw_p);
        if cleaned.is_empty() {
            continue;
        }
        let norm = normalize_self(&cleaned, face.name, face.type_line);

        // Structured multi-section headers: Class level, Leveler band/stats, Unfinity Sticker option
        if let Some((cost_str, level)) = split_class_level_line(&norm) {
            match parse_cost_list(cost_str, vocab) {
                Ok(cost) => out.abilities.push(AbilityAst::ClassLevel { cost, level }),
                Err(reason) => out.failures.push(ParseFailureAst {
                    stage: "cost".to_string(),
                    text: cost_str.to_string(),
                    reason,
                }),
            }
            modal_in_spell = false;
            modal_in_last_ability = false;
            continue;
        }
        if let Some((min_level, max_level)) = parse_leveler_band(&norm) {
            out.statics.push(StaticAbilityAst::LevelerBand {
                min_level,
                max_level,
            });
            modal_in_spell = false;
            modal_in_last_ability = false;
            continue;
        }
        if is_bare_pt(&norm)
            && let Some((power, toughness)) = norm.split_once('/')
        {
            out.statics.push(StaticAbilityAst::LevelerStats {
                power: power.to_string(),
                toughness: toughness.to_string(),
            });
            modal_in_spell = false;
            modal_in_last_ability = false;
            continue;
        }
        if let Some((ticket_cost, payload)) = parse_sticker_option(&norm) {
            out.statics.push(StaticAbilityAst::StickerOption {
                ticket_cost,
                payload,
            });
            modal_in_spell = false;
            modal_in_last_ability = false;
            continue;
        }

        if is_exotic_layout_header(&norm) {
            out.failures.push(ParseFailureAst {
                stage: "layout_line".to_string(),
                text: norm,
                reason: "exotic_layout_header".to_string(),
            });
            modal_in_spell = false;
            modal_in_last_ability = false;
            continue;
        }

        // Modal bullet line (strip optional AFR/UB flavor ability word inside bullet)
        if let Some(bullet_body) = norm.strip_prefix(BULLET) {
            let (_, bullet_clean) = strip_ability_word(bullet_body.trim());
            let (ins, fails) = parse_effect_body(bullet_clean, vocab);
            out.failures.extend(fails);
            if modal_in_spell {
                if let Some(InstructionAst::Modal { modes, .. }) = out.spell_effects.last_mut() {
                    modes.push(ModalModeAst { body: ins });
                }
            } else if modal_in_last_ability {
                if let Some(ab) = out.abilities.last_mut() {
                    match ab {
                        AbilityAst::Activated { body, .. }
                        | AbilityAst::Triggered { body, .. }
                        | AbilityAst::Loyalty { body, .. }
                        | AbilityAst::SagaChapter { body, .. } => {
                            if let Some(InstructionAst::Modal { modes, .. }) = body.last_mut() {
                                modes.push(ModalModeAst { body: ins });
                            }
                        }
                        AbilityAst::ClassLevel { .. } => {}
                    }
                }
            } else {
                out.spell_effects.extend(ins);
            }
            continue;
        }
        modal_in_spell = false;
        modal_in_last_ability = false;

        // Saga chapter (strip optional flavor ability word like "Gungnir — ...")
        if is_saga && let Some((chapters, body_str)) = split_saga_line(&norm) {
            let (_, chapter_clean) = strip_ability_word(body_str.trim());
            let (ins, fails) = parse_effect_body(chapter_clean, vocab);
            modal_in_last_ability = matches!(ins.last(), Some(InstructionAst::Modal { .. }));
            out.abilities.push(AbilityAst::SagaChapter {
                chapters,
                body: ins,
            });
            out.failures.extend(fails);
            continue;
        }

        // Loyalty ability (strip optional flavor ability word)
        if is_planeswalker && let Some((delta, body_str)) = split_loyalty_line(&norm) {
            let (_, loyalty_clean) = strip_ability_word(body_str.trim());
            let (ins, fails) = parse_effect_body(loyalty_clean, vocab);
            modal_in_last_ability = matches!(ins.last(), Some(InstructionAst::Modal { .. }));
            out.abilities.push(AbilityAst::Loyalty { delta, body: ins });
            out.failures.extend(fails);
            continue;
        }

        // Spacecraft station band (`12+ | {1}{R}: ...`) & D20 die-roll band (`20 | ...`, `1-9 | ...`)
        if let Some((left, right)) = norm.split_once(" | ") {
            let lt = left.trim();
            if let Some(num_str) = lt.strip_suffix('+')
                && let Ok(min_charge) = num_str.trim().parse::<u8>()
            {
                out.statics.push(StaticAbilityAst::StationBand {
                    min_charge,
                    payload: right.trim().to_string(),
                });
                continue;
            }
            let is_die_band = !lt.is_empty()
                && lt.chars().count() <= 7
                && lt
                    .chars()
                    .all(|c| c.is_ascii_digit() || c == '-' || c == '\u{2013}');
            if is_die_band && lt.chars().any(|c| c.is_ascii_digit()) && !right.trim().is_empty() {
                out.statics.push(StaticAbilityAst::DieRollBand {
                    roll: lt.to_string(),
                    payload: right.trim().to_string(),
                });
                continue;
            }
        }

        let (ability_word, body_for_parse) = strip_ability_word(&norm);

        // MKM Case "To solve — <condition>" static requirement
        if ability_word
            .as_deref()
            .is_some_and(|w| w.eq_ignore_ascii_case("to solve"))
            && let Ok(cond) = crate::filter::parse_condition(body_for_parse, vocab)
        {
            out.statics.push(StaticAbilityAst::RuleOrCombatRestriction {
                subject: None,
                rule: "CaseToSolve".to_string(),
                filter: None,
                condition: Some(Box::new(cond)),
            });
            continue;
        }

        // Keyword line
        if let Some((kws, k_statics)) = try_parse_keyword_line(body_for_parse, vocab) {
            out.keywords.extend(kws);
            out.statics.extend(k_statics);
            continue;
        }

        // Triggered ability (with Oxford-comma-aware header/body split!)
        if body_for_parse.starts_with("When ")
            || body_for_parse.starts_with("Whenever ")
            || body_for_parse.starts_with("At the beginning of ")
            || body_for_parse.starts_with("At the end of ")
            || body_for_parse.starts_with("At end of combat,")
        {
            let find_clause_comma = |text: &str, from: usize| -> Option<usize> {
                let bytes = text.as_bytes();
                let mut i = from;
                while i < bytes.len() {
                    if bytes[i] == b',' {
                        let numeric_comma = i > 0
                            && i + 1 < bytes.len()
                            && bytes[i - 1].is_ascii_digit()
                            && bytes[i + 1].is_ascii_digit();
                        if !numeric_comma {
                            return Some(i);
                        }
                    }
                    i += 1;
                }
                None
            };
            let low_b = body_for_parse.to_ascii_lowercase();
            let primary_split = if let Some(idx1) = low_b.find(", if ") {
                find_clause_comma(body_for_parse, idx1 + 5).unwrap_or(idx1)
            } else {
                find_clause_comma(body_for_parse, 0).unwrap_or(body_for_parse.len())
            };
            let split_at_pos = |pos: usize| -> (&str, &str) {
                if pos < body_for_parse.len() {
                    (
                        body_for_parse[..pos].trim(),
                        body_for_parse[pos + 1..].trim(),
                    )
                } else {
                    (body_for_parse, "")
                }
            };
            let (mut header, mut body) = split_at_pos(primary_split);
            let mut trig_res = parse_trigger_header(header, vocab);
            let (mut ins, mut fails) = parse_effect_body(body, vocab);
            // If `, if ` appeared inside the effect body (e.g. "Whenever ~ deals combat damage to a player, until your next turn, if ..."),
            // also try the very first clause comma before `, if `!
            if (trig_res.is_err() || !fails.is_empty())
                && let Some(first_c) = find_clause_comma(body_for_parse, 0)
                && first_c < primary_split
            {
                let (cand_h, cand_b) = split_at_pos(first_c);
                if let Ok(t) = parse_trigger_header(cand_h, vocab) {
                    let (cand_ins, cand_fails) = parse_effect_body(cand_b, vocab);
                    if cand_fails.is_empty() || trig_res.is_err() {
                        header = cand_h;
                        body = cand_b;
                        trig_res = Ok(t);
                        ins = cand_ins;
                        fails = cand_fails;
                    }
                }
            }
            // If primary comma split failed EITHER trigger header OR effect body (e.g. Oxford comma
            // inside trigger filter: "Whenever you cast an artifact, instant, or sorcery spell, ..."),
            // scan subsequent commas to find the true header/body boundary!
            if (trig_res.is_err() || !fails.is_empty()) && primary_split < body_for_parse.len() {
                let mut cursor = primary_split + 1;
                let mut tried = 0usize;
                while let Some(pos) = find_clause_comma(body_for_parse, cursor) {
                    cursor = pos + 1;
                    tried += 1;
                    if tried > 5 {
                        break;
                    }
                    let (cand_h, cand_b) = split_at_pos(pos);
                    if let Ok(t) = parse_trigger_header(cand_h, vocab) {
                        let (cand_ins, cand_fails) = parse_effect_body(cand_b, vocab);
                        if cand_fails.is_empty() || trig_res.is_err() {
                            header = cand_h;
                            body = cand_b;
                            trig_res = Ok(t);
                            ins = cand_ins;
                            fails = cand_fails;
                            if fails.is_empty() {
                                break;
                            }
                        }
                    }
                }
            }
            let _ = body;
            match trig_res {
                Ok(trigger) => {
                    modal_in_last_ability =
                        matches!(ins.last(), Some(InstructionAst::Modal { .. }));
                    out.abilities.push(AbilityAst::Triggered {
                        ability_word,
                        trigger,
                        body: ins,
                    });
                }
                Err(reason) => out.failures.push(ParseFailureAst {
                    stage: "trigger".to_string(),
                    text: header.to_string(),
                    reason,
                }),
            }
            out.failures.extend(fails);
            continue;
        }

        // Activated ability (on permanents, OR on instants/sorceries when cost strictly parses,
        // e.g. Magma Opus, Summon the School, Lightning Storm)
        if let Some((cost_str, eff_str)) = split_cost_and_effect(body_for_parse) {
            if !is_spell {
                let (ins, fails) = parse_effect_body(eff_str, vocab);
                match parse_cost_list(cost_str, vocab) {
                    Ok(cost) => {
                        modal_in_last_ability =
                            matches!(ins.last(), Some(InstructionAst::Modal { .. }));
                        out.abilities.push(AbilityAst::Activated {
                            ability_word,
                            zone: None,
                            cost,
                            body: ins,
                        });
                    }
                    Err(reason) => out.failures.push(ParseFailureAst {
                        stage: "cost".to_string(),
                        text: cost_str.to_string(),
                        reason,
                    }),
                }
                out.failures.extend(fails);
                continue;
            } else if let Ok(cost) = parse_cost_list(cost_str, vocab) {
                let (ins, fails) = parse_effect_body(eff_str, vocab);
                modal_in_last_ability = matches!(ins.last(), Some(InstructionAst::Modal { .. }));
                out.abilities.push(AbilityAst::Activated {
                    ability_word,
                    zone: None,
                    cost,
                    body: ins,
                });
                out.failures.extend(fails);
                continue;
            }
        }

        // Spell body or spell static/cost mod
        if is_spell {
            let low_b = body_for_parse.to_ascii_lowercase();
            let is_spell_static = low_b.starts_with("as an additional cost")
                || low_b.starts_with("a deck can have any number of cards named ")
                || low_b.starts_with("flashback ")
                || (!body_for_parse.contains(". ")
                    && (low_b.contains(" costs ") || low_b.contains(" cost "))
                    && (low_b.contains("less to cast") || low_b.contains("more to cast")))
                || low_b.trim_end_matches('.') == "~ can't be countered";
            if is_spell_static {
                match parse_static_or_replacement(body_for_parse, vocab) {
                    Ok(StaticOrReplacement::Static(st)) => out.statics.push(st),
                    Ok(StaticOrReplacement::Replacement(rp)) => out.replacements.push(rp),
                    Err(reason) => out.failures.push(ParseFailureAst {
                        stage: "spell_static".to_string(),
                        text: body_for_parse.to_string(),
                        reason,
                    }),
                }
                continue;
            }
            let (ins, fails) = parse_effect_body(body_for_parse, vocab);
            modal_in_spell = matches!(ins.last(), Some(InstructionAst::Modal { .. }));
            out.spell_effects.extend(ins);
            out.failures.extend(fails);
            continue;
        }

        // Static / Replacement on permanent, Plane, Scheme, or Conspiracy
        match parse_static_or_replacement(body_for_parse, vocab) {
            Ok(StaticOrReplacement::Static(StaticAbilityAst::ManualTabletopStatic { .. }))
                if !options.allow_tabletop_honor_system =>
            {
                out.failures.push(ParseFailureAst {
                    stage: "static".to_string(),
                    text: body_for_parse.to_string(),
                    reason: format!("unparsed_static:{body_for_parse}"),
                });
            }
            Ok(StaticOrReplacement::Static(st)) => out.statics.push(st),
            Ok(StaticOrReplacement::Replacement(rp)) => out.replacements.push(rp),
            Err(reason) => {
                // Multi-sentence static/replacement paragraph (e.g. "~ enters tapped. As it enters, choose a color.")
                let sents = split_sentences(body_for_parse);
                let mut multi_ok = sents.len() >= 2;
                let mut sub_kws = Vec::new();
                let mut sub_statics = Vec::new();
                let mut sub_repls = Vec::new();
                if multi_ok {
                    for sent in &sents {
                        if let Some((kws, kst)) = try_parse_keyword_line(sent, vocab) {
                            sub_kws.extend(kws);
                            sub_statics.extend(kst);
                            continue;
                        }
                        match parse_static_or_replacement(sent, vocab) {
                            Ok(StaticOrReplacement::Static(
                                StaticAbilityAst::ManualTabletopStatic { .. },
                            )) if !options.allow_tabletop_honor_system => {
                                multi_ok = false;
                                break;
                            }
                            Ok(StaticOrReplacement::Static(st)) => sub_statics.push(st),
                            Ok(StaticOrReplacement::Replacement(rp)) => sub_repls.push(rp),
                            Err(_) => {
                                // Allow imperative follow-up sentences ("If you do, ...", "If you don't, ...",
                                // "When you do, ...", "Otherwise, ...") when at least one static/replacement
                                // has already parsed in this paragraph.
                                let sl = sent.trim().to_ascii_lowercase();
                                let is_followup_lead = sl.starts_with("if you do, ")
                                    || sl.starts_with("if you don't, ")
                                    || sl.starts_with("if you can't, ")
                                    || sl.starts_with("if you cast ")
                                    || sl.starts_with("when you do, ")
                                    || sl.starts_with("if x is ")
                                    || sl.starts_with("then ");
                                if (!sub_statics.is_empty() || !sub_repls.is_empty())
                                    && is_followup_lead
                                {
                                    let (ins, fails) = parse_effect_body(sent, vocab);
                                    if fails.is_empty()
                                        && !ins.is_empty()
                                        && (options.allow_tabletop_honor_system
                                            || !ins.iter().any(instruction_uses_honor_system))
                                    {
                                        sub_statics.push(StaticAbilityAst::FollowUp(ins));
                                        continue;
                                    }
                                }
                                multi_ok = false;
                                break;
                            }
                        }
                    }
                }
                if multi_ok {
                    out.keywords.extend(sub_kws);
                    out.statics.extend(sub_statics);
                    out.replacements.extend(sub_repls);
                    continue;
                }

                // Attraction Visit effect paragraph (non-spell permanent with Visit die-roll effect)
                if is_attraction {
                    let (ins, fails) = parse_effect_body(body_for_parse, vocab);
                    if fails.is_empty() && !ins.is_empty() {
                        out.abilities.push(AbilityAst::Triggered {
                            ability_word: Some("Visit".to_string()),
                            trigger: ast::TriggerSpecAst {
                                events: vec![TriggerEventAst::RollDie {
                                    player: ast::PlayerRefAst::You,
                                    exact_result: None,
                                }],
                                intervening_if: None,
                            },
                            body: ins,
                        });
                        continue;
                    }
                }

                // Gotcha ability word without "When/Whenever" prefix ("Gotcha - If an opponent audibly ...")
                if options.allow_tabletop_honor_system
                    && ability_word
                        .as_deref()
                        .is_some_and(|w| w.eq_ignore_ascii_case("gotcha"))
                {
                    let (trigger_part, effect_part) = body_for_parse
                        .split_once(", ")
                        .unwrap_or((body_for_parse, ""));
                    let (ins, fails) = parse_effect_body(effect_part, vocab);
                    out.abilities.push(AbilityAst::Triggered {
                        ability_word,
                        trigger: ast::TriggerSpecAst {
                            events: vec![TriggerEventAst::ManualTabletopClaim {
                                prompt: trigger_part.to_string(),
                                any_player_can_trigger: true,
                            }],
                            intervening_if: None,
                        },
                        body: ins,
                    });
                    out.failures.extend(fails);
                } else {
                    out.failures.push(ParseFailureAst {
                        stage: "static".to_string(),
                        text: body_for_parse.to_string(),
                        reason,
                    });
                }
            }
        }
    }

    if !options.allow_tabletop_honor_system && face_uses_honor_system(&out) {
        // Guard: reject any ManualTabletop* AST nodes that slipped through on non-Un cards.
        out.failures.push(ParseFailureAst {
            stage: "honor_system_guard".to_string(),
            text: face.name.to_string(),
            reason: "tabletop_honor_system_disabled_for_standard_cards".to_string(),
        });
    }

    if is_conspiracy
        && !out
            .statics
            .iter()
            .any(|s| matches!(s, StaticAbilityAst::ConspiracyAgenda { .. }))
    {
        out.statics.push(StaticAbilityAst::ConspiracyAgenda {
            hidden: false,
            named_slots: 0,
        });
    }

    out
}

fn static_uses_honor_system(st: &StaticAbilityAst) -> bool {
    match st {
        StaticAbilityAst::ManualTabletopStatic { .. } => true,
        StaticAbilityAst::GrantAbilityText {
            compiled: Some(inner),
            ..
        } => face_uses_honor_system(inner),
        StaticAbilityAst::GrantKeywords {
            paired_rule: Some(inner),
            ..
        } => static_uses_honor_system(inner),
        StaticAbilityAst::Otherwise(inner) | StaticAbilityAst::OncePerTurn(inner) => {
            static_uses_honor_system(inner)
        }
        StaticAbilityAst::Compound(items) => items.iter().any(static_uses_honor_system),
        StaticAbilityAst::FollowUp(body) => body.iter().any(instruction_uses_honor_system),
        _ => false,
    }
}

fn face_uses_honor_system(face: &FaceAst) -> bool {
    if face.statics.iter().any(static_uses_honor_system) {
        return true;
    }
    if face.replacements.iter().any(|r| match r {
        ast::ReplacementEffectAst::EventInstead {
            instead_body: Some(body),
            ..
        }
        | ast::ReplacementEffectAst::AsEntersAction(body) => {
            body.iter().any(instruction_uses_honor_system)
        }
        _ => false,
    }) {
        return true;
    }
    if face.spell_effects.iter().any(instruction_uses_honor_system) {
        return true;
    }
    for ab in &face.abilities {
        match ab {
            AbilityAst::Triggered { trigger, body, .. } => {
                if trigger
                    .events
                    .iter()
                    .any(|e| matches!(e, TriggerEventAst::ManualTabletopClaim { .. }))
                {
                    return true;
                }
                if body.iter().any(instruction_uses_honor_system) {
                    return true;
                }
            }
            AbilityAst::Activated { cost, body, .. } => {
                if cost
                    .iter()
                    .any(|c| matches!(c, ast::CostAtomAst::ManualTabletopAction { .. }))
                {
                    return true;
                }
                if body.iter().any(instruction_uses_honor_system) {
                    return true;
                }
            }
            AbilityAst::Loyalty { body, .. } | AbilityAst::SagaChapter { body, .. } => {
                if body.iter().any(instruction_uses_honor_system) {
                    return true;
                }
            }
            AbilityAst::ClassLevel { cost, .. } => {
                if cost
                    .iter()
                    .any(|c| matches!(c, ast::CostAtomAst::ManualTabletopAction { .. }))
                {
                    return true;
                }
            }
        }
    }
    false
}

fn instruction_uses_honor_system(ins: &InstructionAst) -> bool {
    match ins {
        InstructionAst::ManualTabletopAction { .. } => true,
        InstructionAst::Optional(b)
        | InstructionAst::OptionalPlayer { body: b, .. }
        | InstructionAst::IfPreviousSucceeded(b)
        | InstructionAst::IfPreviousFailed(b)
        | InstructionAst::Otherwise(b)
        | InstructionAst::ReplaceEffect(b)
        | InstructionAst::ReflexiveTrigger(b)
        | InstructionAst::BindX { body: b, .. }
        | InstructionAst::ForEachPlayer { body: b, .. }
        | InstructionAst::ForEachObject { body: b, .. }
        | InstructionAst::ForEachCount { body: b, .. }
        | InstructionAst::DelayedTrigger { body: b, .. }
        | InstructionAst::DelayedEventTrigger { body: b, .. } => {
            b.iter().any(instruction_uses_honor_system)
        }
        InstructionAst::Unless {
            body,
            unless_action,
            unless_condition,
            ..
        } => {
            body.iter().any(instruction_uses_honor_system)
                || unless_action
                    .as_ref()
                    .is_some_and(|a| a.iter().any(instruction_uses_honor_system))
                || unless_condition
                    .as_ref()
                    .is_some_and(|c| matches!(**c, ast::ConditionAst::ManualTabletopCheck { .. }))
        }
        InstructionAst::IfCondition {
            condition,
            then_body,
        } => {
            matches!(**condition, ast::ConditionAst::ManualTabletopCheck { .. })
                || then_body.iter().any(instruction_uses_honor_system)
        }
        InstructionAst::GrantAbilityText {
            compiled: Some(inner),
            ..
        } => face_uses_honor_system(inner),
        InstructionAst::OneShotReplacement {
            instead_body: Some(body),
            ..
        } => body.iter().any(instruction_uses_honor_system),
        InstructionAst::TemporaryStatic { static_ability, .. } => {
            static_uses_honor_system(static_ability)
        }
        InstructionAst::Choice(branches) => branches
            .iter()
            .any(|b| b.iter().any(instruction_uses_honor_system)),
        InstructionAst::InteractiveChoice(ast::InteractiveChoiceKind::VillainousChoice {
            options,
            ..
        }) => options
            .iter()
            .any(|opt| opt.iter().any(instruction_uses_honor_system)),
        InstructionAst::Modal { modes, .. } => modes
            .iter()
            .any(|m| m.body.iter().any(instruction_uses_honor_system)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vocab() -> Vocabulary {
        Vocabulary::from_creature_members(&[
            "Human".to_string(),
            "Soldier".to_string(),
            "Wizard".to_string(),
            "Elemental".to_string(),
            "Demon".to_string(),
            "Beast".to_string(),
            "Goblin".to_string(),
            "Elf".to_string(),
            "Ally".to_string(),
            "Spirit".to_string(),
        ])
    }

    #[test]
    fn compiles_gatecrash_bloodrush_and_shockland_and_modal() {
        let vocab = test_vocab();
        let rubblehulk = CardInput {
            oracle_id: "rubblehulk",
            name: "Rubblehulk",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Rubblehulk",
                type_line: "Creature \u{2014} Elemental",
                oracle_text: "Rubblehulk's power and toughness are each equal to the number of lands you control.\nBloodrush \u{2014} {1}{R}{G}, Discard Rubblehulk: Target attacking creature gets +X/+X until end of turn, where X is the number of lands you control.",
            }],
        };
        let r_ast = compile_card(&rubblehulk, &vocab);
        assert!(
            r_ast.strictly_compiled,
            "failures: {:?}",
            r_ast.faces[0].failures
        );
        assert_eq!(r_ast.faces[0].statics.len(), 1);
        assert_eq!(r_ast.faces[0].abilities.len(), 1);

        let breeding_pool = CardInput {
            oracle_id: "breeding-pool",
            name: "Breeding Pool",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Breeding Pool",
                type_line: "Land \u{2014} Forest Island",
                oracle_text: "({T}: Add {G} or {U}.)\nAs Breeding Pool enters, you may pay 2 life. If you don't, it enters tapped.",
            }],
        };
        let bp_ast = compile_card(&breeding_pool, &vocab);
        assert!(
            bp_ast.strictly_compiled,
            "failures: {:?}",
            bp_ast.faces[0].failures
        );
        assert_eq!(bp_ast.faces[0].replacements.len(), 1);

        let boros_charm = CardInput {
            oracle_id: "boros-charm",
            name: "Boros Charm",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Boros Charm",
                type_line: "Instant",
                oracle_text: "Choose one \u{2014}\n\u{2022} Boros Charm deals 4 damage to target player or planeswalker.\n\u{2022} Permanents you control gain indestructible until end of turn.\n\u{2022} Target creature gains double strike until end of turn.",
            }],
        };
        let bc_ast = compile_card(&boros_charm, &vocab);
        assert!(
            bc_ast.strictly_compiled,
            "failures: {:?}",
            bc_ast.faces[0].failures
        );
        assert!(matches!(
            bc_ast.faces[0].spell_effects.first(),
            Some(InstructionAst::Modal { modes, .. }) if modes.len() == 3
        ));
    }

    #[test]
    fn compiles_planechase_archenemy_conspiracy_and_tabletop_uncard() {
        let vocab = test_vocab();

        let plane = CardInput {
            oracle_id: "plane-1",
            name: "Gavony",
            layout: "planar",
            faces: vec![FaceInput {
                name: "Gavony",
                type_line: "Plane \u{2014} Innistrad",
                oracle_text: "All creatures have vigilance.\nWhenever chaos ensues, you gain 4 life.",
            }],
        };
        let p_ast = compile_card(&plane, &vocab);
        assert!(
            p_ast.strictly_compiled,
            "failures: {:?}",
            p_ast.faces[0].failures
        );

        let scheme = CardInput {
            oracle_id: "scheme-1",
            name: "What's Yours Is Now Mine",
            layout: "scheme",
            faces: vec![FaceInput {
                name: "What's Yours Is Now Mine",
                type_line: "Scheme",
                oracle_text: "When you set this scheme in motion, gain control of target creature an opponent controls and untap it.",
            }],
        };
        let s_ast = compile_card(&scheme, &vocab);
        assert!(
            s_ast.strictly_compiled,
            "failures: {:?}",
            s_ast.faces[0].failures
        );

        let conspiracy = CardInput {
            oracle_id: "consp-1",
            name: "Advantageous Proclamation",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Advantageous Proclamation",
                type_line: "Conspiracy",
                oracle_text: "Hidden agenda (Start the game with this conspiracy face down in the command zone and secretly name a card.)",
            }],
        };
        let c_ast = compile_card(&conspiracy, &vocab);
        assert!(
            c_ast.strictly_compiled,
            "failures: {:?}",
            c_ast.faces[0].failures
        );
        assert!(matches!(
            c_ast.faces[0].statics.first(),
            Some(StaticAbilityAst::ConspiracyAgenda {
                hidden: true,
                named_slots: 1
            })
        ));

        let stop_that = CardInput {
            oracle_id: "stop-that",
            name: "Stop That",
            layout: "normal",
            faces: vec![FaceInput {
                name: "Stop That",
                type_line: "Sorcery",
                oracle_text: "Target player discards a card.\nGotcha \u{2014} If an opponent audibly flicks the cards in their hand, you may say \"Gotcha!\" If you do, return Stop That from your graveyard to your hand.",
            }],
        };
        let st_ast = compile_card_with_options(
            &stop_that,
            &vocab,
            CompileOptions {
                allow_tabletop_honor_system: true,
            },
        );
        assert!(st_ast.uses_tabletop_honor_system);
    }
}
