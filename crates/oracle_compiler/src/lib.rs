//! Standalone zero-unsafe Phase 4 Oracle Clause Compiler for Spellwake.
//!
//! Compiles Magic Oracle text across all 33,138 real-paper cards (Base Constructed/Commander,
//! Planechase, Archenemy, Conspiracy, and Paper Un-cards with Tabletop Honor-System primitives)
//! into strongly-typed, `serde`-serializable ASTs ready for thin lowering into `spellwake-engine`.

pub mod ast;
pub mod audit;
pub mod clauses;
pub(crate) mod combinators;
pub mod filter;
pub mod keywords;
pub mod preprocess;
pub mod standardize;
pub mod typed_ir;
pub mod v2;
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
    split_saga_line, split_sentences, strip_ability_word, strip_reminder, strip_reminder_multiline,
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

/// Normalize nested single-quoted granted abilities (e.g. `has '{T}: Add {G}{G}.'`)
/// into double quotes without disturbing English apostrophes (`owner's`, `can't`, `~'s`).
fn normalize_nested_single_quotes(text: &str) -> String {
    if !text.contains('\'') || text.contains('"') {
        return text.to_string();
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut in_sq = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' {
            if !in_sq {
                let prev_space_or_start = i == 0 || bytes[i - 1] == b' ';
                let next_starts_ability = bytes
                    .get(i + 1)
                    .is_some_and(|&nb| nb == b'{' || nb.is_ascii_uppercase() || nb == b'~');
                if prev_space_or_start && next_starts_ability {
                    in_sq = true;
                    out.push('"');
                    i += 1;
                    continue;
                }
            } else {
                let next_ends_quote = bytes
                    .get(i + 1)
                    .is_none_or(|&nb| matches!(nb, b' ' | b',' | b'.' | b';'));
                if next_ends_quote {
                    in_sq = false;
                    out.push('"');
                    i += 1;
                    continue;
                }
            }
        }
        // Preserve multi-byte UTF-8 safely
        if let Some(ch) = text[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    if in_sq {
        // Unbalanced single quote: revert to original untouched text
        text.to_string()
    } else {
        out
    }
}

/// Compile a spacecraft station band (`min_charge+ | payload`) into `CompiledStationBand` when possible.
#[must_use]
pub(crate) fn build_station_band(
    min_charge: u8,
    payload: String,
    vocab: &Vocabulary,
) -> StaticAbilityAst {
    let trimmed = payload.trim();
    if let Some((p, t)) = trimmed.split_once('/')
        && is_bare_pt(trimmed)
    {
        let p = p.to_string();
        let t = t.to_string();
        return StaticAbilityAst::CompiledStationBand {
            min_charge,
            payload,
            power: Some(p),
            toughness: Some(t),
            compiled: None,
        };
    }
    if let Some(compiled) = compile_granted_ability(trimmed, vocab) {
        return StaticAbilityAst::CompiledStationBand {
            min_charge,
            payload,
            power: None,
            toughness: None,
            compiled: Some(compiled),
        };
    }
    StaticAbilityAst::StationBand {
        min_charge,
        payload,
    }
}

/// Compile a die-roll band (`roll | payload`) into `CompiledDieRollBand` when possible.
#[must_use]
pub(crate) fn build_die_roll_band(
    roll: String,
    payload: String,
    vocab: &Vocabulary,
) -> StaticAbilityAst {
    let trimmed = payload.trim();
    let (body, fails) = parse_effect_body(trimmed, vocab);
    if fails.is_empty() && !body.is_empty() {
        return StaticAbilityAst::CompiledDieRollBand {
            roll,
            payload,
            body,
        };
    }
    StaticAbilityAst::DieRollBand { roll, payload }
}

/// Compile an Unfinity sticker option (`{TK}... — payload`) into `CompiledStickerOption` when possible.
#[must_use]
pub(crate) fn build_sticker_option(
    ticket_cost: u8,
    payload: String,
    vocab: &Vocabulary,
) -> StaticAbilityAst {
    let trimmed = payload.trim();
    if let Some((p, t)) = trimmed.split_once('/')
        && is_bare_pt(trimmed)
    {
        let p = p.to_string();
        let t = t.to_string();
        return StaticAbilityAst::CompiledStickerOption {
            ticket_cost,
            payload,
            power: Some(p),
            toughness: Some(t),
            compiled: None,
        };
    }
    if let Some(compiled) = compile_granted_ability(trimmed, vocab) {
        return StaticAbilityAst::CompiledStickerOption {
            ticket_cost,
            payload,
            power: None,
            toughness: None,
            compiled: Some(compiled),
        };
    }
    StaticAbilityAst::StickerOption {
        ticket_cost,
        payload,
    }
}

/// Recursively compile a quoted granted ability string into a typed `FaceAst` fragment.
/// Returns `Some` only when the quoted ability compiles cleanly with zero failures.
#[must_use]
pub fn compile_granted_ability(ability_text: &str, vocab: &Vocabulary) -> Option<Box<FaceAst>> {
    let trimmed = ability_text.trim().trim_end_matches(',');
    if trimmed.is_empty() {
        return None;
    }
    // Token descriptions are normalized to lowercase before their quoted abilities
    // reach this boundary. Restore the paragraph initial expected by face dispatch.
    let normalized = normalize_nested_single_quotes(trimmed)
        .lines()
        .map(|line| {
            let mut line = line.to_string();
            if let Some(initial) = line.get_mut(..1) {
                initial.make_ascii_uppercase();
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");
    let first_ch = normalized.chars().next();
    let looks_loyalty = normalized.starts_with('[')
        || matches!(first_ch, Some('+' | '-' | '0' | '\u{2212}')) && normalized.contains(':');
    let candidate_types: &[&str] = if looks_loyalty {
        &["Planeswalker", "Creature", "Artifact", "Land", "Instant"]
    } else if normalized.to_ascii_lowercase().starts_with("equip") {
        &[
            "Artifact \u{2014} Equipment",
            "Creature",
            "Artifact",
            "Land",
            "Instant",
        ]
    } else {
        &["Creature", "Planeswalker", "Artifact", "Land", "Instant"]
    };
    for &type_line in candidate_types {
        let face = compile_face_with_options(
            FaceInput {
                name: "",
                type_line,
                oracle_text: &normalized,
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
            return Some(Box::new(face));
        }
    }
    None
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
    let is_class = face.type_line.contains("Class");
    let is_spacecraft = face.type_line.contains("Spacecraft") || face.type_line.contains("Planet");

    fn parse_die_roll_band(text: &str) -> Option<(String, String)> {
        let (left, right) = text
            .split_once(" | ")
            .or_else(|| text.split_once('\u{2014}'))?;
        let lt = left.trim();
        let rt = right.trim();
        let is_die_band = !lt.is_empty()
            && lt.chars().count() <= 7
            && lt.chars().any(|c| c.is_ascii_digit())
            && lt.chars().all(|c| {
                c.is_ascii_digit() || c == '-' || c == '\u{2013}' || c == '\u{2014}' || c == '+'
            });
        if is_die_band && !rt.is_empty() {
            Some((lt.to_string(), rt.to_string()))
        } else {
            None
        }
    }

    fn promote_as_enters_modal(rp: &mut ast::ReplacementEffectAst, vocab: &Vocabulary) -> bool {
        let modal_header = match rp {
            ast::ReplacementEffectAst::AsEntersChoose { choice }
                if choice.trim_end().ends_with('\u{2014}') =>
            {
                Some(format!("Choose {choice}"))
            }
            _ => None,
        };
        if let Some(header) = modal_header {
            let (mut ins, fails) = parse_effect_body(&header, vocab);
            if fails.is_empty() && find_last_modal_modes_mut(&mut ins).is_some() {
                *rp = ast::ReplacementEffectAst::AsEntersAction(ins);
                return true;
            }
        }
        if let ast::ReplacementEffectAst::AsEntersAction(body) = rp {
            return find_last_modal_modes_mut(body).is_some();
        }
        false
    }

    fn find_last_modal_modes_mut(ins: &mut [InstructionAst]) -> Option<&mut Vec<ModalModeAst>> {
        for op in ins.iter_mut().rev() {
            match op {
                InstructionAst::AbilityRestriction { .. }
                | InstructionAst::AllowRepeatModes
                | InstructionAst::ModalModifier(_) => {
                    continue;
                }
                InstructionAst::BindX { body, .. } if body.is_empty() => continue,
                InstructionAst::Modal { modes, .. } => return Some(modes),
                InstructionAst::Optional(inner)
                | InstructionAst::OptionalPlayer { body: inner, .. }
                | InstructionAst::IfCondition {
                    then_body: inner, ..
                }
                | InstructionAst::BindX { body: inner, .. }
                | InstructionAst::BindXY { body: inner, .. }
                | InstructionAst::IfPreviousSucceeded(inner)
                | InstructionAst::IfPreviousFailed(inner)
                | InstructionAst::Otherwise(inner)
                | InstructionAst::ReplaceEffect(inner)
                | InstructionAst::ReflexiveTrigger(inner)
                | InstructionAst::DelayedTrigger { body: inner, .. }
                | InstructionAst::DelayedEventTrigger { body: inner, .. }
                | InstructionAst::ForEachPlayer { body: inner, .. }
                | InstructionAst::ForEachObject { body: inner, .. }
                | InstructionAst::ForEachCount { body: inner, .. }
                | InstructionAst::ForEachSubjectPlayer { body: inner, .. }
                | InstructionAst::ForEachColorAmong { body: inner, .. }
                | InstructionAst::ForAsLongAs { body: inner, .. }
                | InstructionAst::WhenYouNext { body: inner, .. }
                | InstructionAst::TemporarySpecialAction { body: inner, .. }
                | InstructionAst::Unless { body: inner, .. } => {
                    if let Some(modes) = find_last_modal_modes_mut(inner) {
                        return Some(modes);
                    }
                }
                _ => {}
            }
        }
        None
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ModalTarget {
        None,
        Spell,
        LastAbility,
        LastReplacement,
    }
    let mut modal_target = ModalTarget::None;
    let mut last_replacement_is_named_choice = false;
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum ActiveSection {
        Base,
        LevelBand,
        ClassLevel,
        CaseSolved,
    }
    let mut section = ActiveSection::Base;

    let preprocessed_oracle = strip_reminder_multiline(face.oracle_text);
    for raw_p in preprocessed_oracle.split('\n') {
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
                Ok(cost) => {
                    out.class_levels.push(ast::ClassLevelSectionAst {
                        level,
                        cost,
                        ..ast::ClassLevelSectionAst::default()
                    });
                    section = ActiveSection::ClassLevel;
                }
                Err(reason) => out.failures.push(ParseFailureAst {
                    stage: "cost".to_string(),
                    text: cost_str.to_string(),
                    reason,
                }),
            }
            modal_target = ModalTarget::None;
            continue;
        }
        if is_class && norm == "Level 1" {
            out.class_levels.push(ast::ClassLevelSectionAst {
                level: 1,
                cost: Vec::new(),
                ..ast::ClassLevelSectionAst::default()
            });
            section = ActiveSection::ClassLevel;
            modal_target = ModalTarget::None;
            continue;
        }
        if let Some((min_level, max_level)) = parse_leveler_band(&norm) {
            out.level_bands.push(ast::LevelerBandAst {
                min_level,
                max_level,
                ..ast::LevelerBandAst::default()
            });
            section = ActiveSection::LevelBand;
            modal_target = ModalTarget::None;
            continue;
        }
        if is_bare_pt(&norm)
            && let Some((power, toughness)) = norm.split_once('/')
        {
            if section == ActiveSection::LevelBand
                && let Some(band) = out.level_bands.last_mut()
            {
                band.power = Some(power.to_string());
                band.toughness = Some(toughness.to_string());
            } else {
                out.statics.push(StaticAbilityAst::LevelerStats {
                    power: power.to_string(),
                    toughness: toughness.to_string(),
                });
            }
            modal_target = ModalTarget::None;
            continue;
        }
        if let Some((ticket_cost, payload)) = parse_sticker_option(&norm) {
            out.statics
                .push(build_sticker_option(ticket_cost, payload, vocab));
            modal_target = ModalTarget::None;
            continue;
        }

        if is_exotic_layout_header(&norm) {
            // Cluster 21: Prototype `{M} - P/T` header compiles losslessly into PrototypeStats
            if let Some((left, right)) = norm.split_once(preprocess::EM_DASH)
                && preprocess::is_mana_sequence(left.trim())
                && is_bare_pt(right.trim())
                && let (Ok(cost), Some((power, toughness))) = (
                    parse_cost_list(left.trim(), vocab),
                    right.trim().split_once('/'),
                )
            {
                out.statics.push(StaticAbilityAst::PrototypeStats {
                    cost,
                    power: power.to_string(),
                    toughness: toughness.to_string(),
                });
                modal_target = ModalTarget::None;
                last_replacement_is_named_choice = false;
                continue;
            }
            out.failures.push(ParseFailureAst {
                stage: "layout_line".to_string(),
                text: norm,
                reason: "exotic_layout_header".to_string(),
            });
            modal_target = ModalTarget::None;
            last_replacement_is_named_choice = false;
            continue;
        }

        // Modal bullet line (bullet OR Bloomburrow pawprint modes "{P} <em-dash> ...", "{P}{P} <em-dash> ...", "{P}{P}{P} <em-dash> ...")
        let pawprint_bullet: Option<(&str, &str)> = if modal_target != ModalTarget::None
            && norm.starts_with("{P}")
            && let Some((paw_lhs, paw_rhs)) = norm.split_once(preprocess::EM_DASH)
        {
            let pl = paw_lhs.trim();
            if !pl.is_empty() && pl.replace("{P}", "").is_empty() {
                Some((pl, paw_rhs.trim()))
            } else {
                None
            }
        } else {
            None
        };
        if let Some((paw_label, paw_body)) = pawprint_bullet {
            let (ins, fails) = parse_effect_body(paw_body, vocab);
            out.failures.extend(fails);
            let mode_ast = ModalModeAst {
                body: ins,
                label: Some(paw_label.to_string()),
                granted: None,
            };
            match modal_target {
                ModalTarget::Spell => {
                    if let Some(modes) = find_last_modal_modes_mut(&mut out.spell_effects) {
                        modes.push(mode_ast);
                    }
                }
                ModalTarget::LastAbility => {
                    if let Some(ab) = out.abilities.last_mut() {
                        match ab {
                            AbilityAst::Activated { body, .. }
                            | AbilityAst::Triggered { body, .. }
                            | AbilityAst::Loyalty { body, .. }
                            | AbilityAst::SagaChapter { body, .. } => {
                                if let Some(modes) = find_last_modal_modes_mut(body) {
                                    modes.push(mode_ast);
                                }
                            }
                            AbilityAst::ClassLevel { .. } => {}
                        }
                    }
                }
                ModalTarget::LastReplacement | ModalTarget::None => {}
            }
            continue;
        }
        if let Some(bullet_body) = norm.strip_prefix(BULLET) {
            if modal_target == ModalTarget::None
                && let Some((roll, payload)) = parse_die_roll_band(bullet_body.trim())
            {
                let band = build_die_roll_band(roll, payload, vocab);
                match section {
                    ActiveSection::Base => out.statics.push(band),
                    ActiveSection::LevelBand => {
                        if let Some(lb) = out.level_bands.last_mut() {
                            lb.statics.push(band);
                        } else {
                            out.statics.push(band);
                        }
                    }
                    ActiveSection::ClassLevel => {
                        if let Some(cl) = out.class_levels.last_mut() {
                            cl.statics.push(band);
                        } else {
                            out.statics.push(band);
                        }
                    }
                    ActiveSection::CaseSolved => out.case_solved.statics.push(band),
                }
                continue;
            }
            // Cluster 1: Promote `As ~ enters, choose Khans or Dragons.` when bullets follow
            if modal_target == ModalTarget::None && last_replacement_is_named_choice {
                let target_repls: &mut Vec<ast::ReplacementEffectAst> = match section {
                    ActiveSection::Base => &mut out.replacements,
                    ActiveSection::LevelBand => out
                        .level_bands
                        .last_mut()
                        .map_or(&mut out.replacements, |b| &mut b.replacements),
                    ActiveSection::ClassLevel => out
                        .class_levels
                        .last_mut()
                        .map_or(&mut out.replacements, |c| &mut c.replacements),
                    ActiveSection::CaseSolved => &mut out.case_solved.replacements,
                };
                if let Some(last_rp) = target_repls.last_mut()
                    && let ast::ReplacementEffectAst::AsEntersChoose { choice } = last_rp
                {
                    let ch = choice.clone();
                    *last_rp =
                        ast::ReplacementEffectAst::AsEntersAction(vec![InstructionAst::Modal {
                            choice: ch,
                            modes: Vec::new(),
                        }]);
                    modal_target = ModalTarget::LastReplacement;
                }
            }
            let (bullet_label, bullet_clean) = strip_ability_word(bullet_body.trim());
            let mut granted_face: Option<Box<FaceAst>> = None;
            let (ins, fails) = if modal_target == ModalTarget::LastReplacement
                && let Some(compiled) = compile_granted_ability(bullet_clean, vocab)
            {
                granted_face = Some(compiled);
                (Vec::new(), Vec::new())
            } else {
                parse_effect_body(bullet_clean, vocab)
            };
            out.failures.extend(fails);
            let mode_ast = if let Some(gf) = granted_face {
                ModalModeAst {
                    body: Vec::new(),
                    label: bullet_label,
                    granted: Some(gf),
                }
            } else {
                ModalModeAst {
                    body: ins,
                    label: bullet_label,
                    granted: None,
                }
            };
            match modal_target {
                ModalTarget::Spell => {
                    if let Some(modes) = find_last_modal_modes_mut(&mut out.spell_effects) {
                        modes.push(mode_ast);
                    }
                }
                ModalTarget::LastAbility => {
                    let target_abilities: &mut Vec<AbilityAst> = match section {
                        ActiveSection::Base => &mut out.abilities,
                        ActiveSection::LevelBand => out
                            .level_bands
                            .last_mut()
                            .map_or(&mut out.abilities, |b| &mut b.abilities),
                        ActiveSection::ClassLevel => out
                            .class_levels
                            .last_mut()
                            .map_or(&mut out.abilities, |c| &mut c.abilities),
                        ActiveSection::CaseSolved => &mut out.case_solved.abilities,
                    };
                    if let Some(ab) = target_abilities.last_mut() {
                        match ab {
                            AbilityAst::Activated { body, .. }
                            | AbilityAst::Triggered { body, .. }
                            | AbilityAst::Loyalty { body, .. }
                            | AbilityAst::SagaChapter { body, .. } => {
                                if let Some(modes) = find_last_modal_modes_mut(body) {
                                    modes.push(mode_ast);
                                }
                            }
                            AbilityAst::ClassLevel { .. } => {}
                        }
                    }
                }
                ModalTarget::LastReplacement => {
                    let target_repls: &mut Vec<ast::ReplacementEffectAst> = match section {
                        ActiveSection::Base => &mut out.replacements,
                        ActiveSection::LevelBand => out
                            .level_bands
                            .last_mut()
                            .map_or(&mut out.replacements, |b| &mut b.replacements),
                        ActiveSection::ClassLevel => out
                            .class_levels
                            .last_mut()
                            .map_or(&mut out.replacements, |c| &mut c.replacements),
                        ActiveSection::CaseSolved => &mut out.case_solved.replacements,
                    };
                    if let Some(ast::ReplacementEffectAst::AsEntersAction(body)) =
                        target_repls.last_mut()
                        && let Some(modes) = find_last_modal_modes_mut(body)
                    {
                        modes.push(mode_ast);
                    }
                }
                ModalTarget::None => {
                    if is_spell {
                        out.spell_effects.extend(mode_ast.body);
                    } else {
                        out.failures.push(ParseFailureAst {
                            stage: "modal_bullet".to_string(),
                            text: norm,
                            reason: "orphan_modal_bullet_on_permanent".to_string(),
                        });
                    }
                }
            }
            continue;
        }
        modal_target = ModalTarget::None;
        last_replacement_is_named_choice = false;

        // Saga chapter (strip optional flavor ability word like "Gungnir - ...")
        if is_saga && let Some((chapters, body_str)) = split_saga_line(&norm) {
            let (_, chapter_clean) = strip_ability_word(body_str.trim());
            let (mut ins, fails) = parse_effect_body(chapter_clean, vocab);
            if find_last_modal_modes_mut(&mut ins).is_some() {
                modal_target = ModalTarget::LastAbility;
            }
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
            let (mut ins, fails) = parse_effect_body(loyalty_clean, vocab);
            if find_last_modal_modes_mut(&mut ins).is_some() {
                modal_target = ModalTarget::LastAbility;
            }
            out.abilities.push(AbilityAst::Loyalty { delta, body: ins });
            out.failures.extend(fails);
            continue;
        }

        // Spacecraft station band (`12+ | {1}{R}: ...`) & D20 die-roll band (`20 | ...`, `1-9 | ...`, `15+ | ...`)
        if let Some((left, right)) = norm.split_once(" | ") {
            let lt = left.trim();
            let has_station = is_spacecraft
                || out
                    .keywords
                    .iter()
                    .any(|k| k.name.eq_ignore_ascii_case("station"));
            if has_station
                && let Some(num_str) = lt.strip_suffix('+')
                && let Ok(min_charge) = num_str.trim().parse::<u8>()
            {
                out.statics.push(build_station_band(
                    min_charge,
                    right.trim().to_string(),
                    vocab,
                ));
                continue;
            }
            let is_die_band = (!lt.is_empty()
                && lt.chars().count() <= 7
                && lt.chars().all(|c| {
                    c.is_ascii_digit() || c == '-' || c == '\u{2013}' || c == '\u{2014}' || c == '+'
                }))
                || (lt.ends_with(" or less")
                    && lt
                        .strip_suffix(" or less")
                        .is_some_and(|n| n.trim().parse::<u8>().is_ok()));
            if is_die_band && lt.chars().any(|c| c.is_ascii_digit()) && !right.trim().is_empty() {
                out.statics.push(build_die_roll_band(
                    lt.to_string(),
                    right.trim().to_string(),
                    vocab,
                ));
                continue;
            }
        }
        // Em-dash die-roll band (`1 or 2 — ...`, `4 or 5 — ...` on Comet, Stellar Pup)
        if let Some((left, right)) = norm.split_once(" \u{2014} ")
            && let Some((a, b)) = left.trim().split_once(" or ")
            && a.trim().parse::<u8>().is_ok()
            && b.trim().parse::<u8>().is_ok()
            && !right.trim().is_empty()
        {
            out.statics.push(build_die_roll_band(
                left.trim().to_string(),
                right.trim().to_string(),
                vocab,
            ));
            continue;
        }

        let (ability_word, body_for_parse) = strip_ability_word(&norm);

        // MKM Case "Solved — ..." marks solved-only abilities for this paragraph.
        let is_case_solved_para = ability_word
            .as_deref()
            .is_some_and(|w| w.eq_ignore_ascii_case("solved"));
        let para_section = if is_case_solved_para {
            section = ActiveSection::CaseSolved;
            ActiveSection::CaseSolved
        } else if section == ActiveSection::CaseSolved {
            section = ActiveSection::Base;
            ActiveSection::Base
        } else {
            section
        };

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
        let spell_action_kw = is_spell && {
            let bl = body_for_parse.to_ascii_lowercase();
            bl.starts_with("earthbend ")
                || bl.starts_with("airbend ")
                || bl.starts_with("waterbend ")
        };
        if !spell_action_kw
            && let Some((kws, k_statics)) = try_parse_keyword_line(body_for_parse, vocab)
        {
            match para_section {
                ActiveSection::Base => {
                    out.keywords.extend(kws);
                    out.statics.extend(k_statics);
                }
                ActiveSection::LevelBand => {
                    if let Some(band) = out.level_bands.last_mut() {
                        band.keywords.extend(kws);
                        band.statics.extend(k_statics);
                    }
                }
                ActiveSection::ClassLevel => {
                    if let Some(cl) = out.class_levels.last_mut() {
                        cl.keywords.extend(kws);
                        cl.statics.extend(k_statics);
                    }
                }
                ActiveSection::CaseSolved => {
                    out.case_solved.keywords.extend(kws);
                    out.case_solved.statics.extend(k_statics);
                }
            }
            continue;
        }

        // Triggered ability (with Oxford-comma-aware header/body split!)
        // On instants/sorceries, "At the beginning of ..." / "At end of combat, ..." and
        // "When/Whenever ... this turn / until end of turn / this combat," are delayed
        // triggers created as spell effects UNLESS they explicitly check a non-stack zone
        // or are cast triggers ("When you cast ~").
        let body_low_for_delayed = body_for_parse.to_ascii_lowercase();
        let is_spell_delayed_effect = is_spell
            && (body_for_parse.starts_with("At the beginning of ")
                || body_for_parse.starts_with("At the end of ")
                || body_for_parse.starts_with("At end of combat,")
                || ((body_for_parse.starts_with("When ")
                    || body_for_parse.starts_with("Whenever "))
                    && !body_low_for_delayed.starts_with("when you cast ~")
                    && (body_low_for_delayed.contains("this turn,")
                        || body_low_for_delayed.contains("until end of turn,")
                        || body_low_for_delayed.contains("this combat,"))))
            && !body_low_for_delayed.contains("in your graveyard")
            && !body_low_for_delayed.contains("is exiled")
            && !body_low_for_delayed.contains("from your graveyard");
        if !is_spell_delayed_effect
            && (body_for_parse.starts_with("When ")
                || body_for_parse.starts_with("Whenever ")
                || body_for_parse.starts_with("At the beginning of ")
                || body_for_parse.starts_with("At the end of ")
                || body_for_parse.starts_with("At end of combat"))
        {
            let mut clause_commas: Vec<usize> = Vec::new();
            {
                let bytes = body_for_parse.as_bytes();
                let mut in_quotes = false;
                let mut i = 0;
                while i < bytes.len() {
                    match bytes[i] {
                        b'"' => in_quotes = !in_quotes,
                        b'.' if !in_quotes => {
                            // Never scan trigger-header commas past the end of sentence 1 (fixes Sylvan Library).
                            break;
                        }
                        b',' if !in_quotes => {
                            let numeric_comma = i > 0
                                && i + 1 < bytes.len()
                                && bytes[i - 1].is_ascii_digit()
                                && bytes[i + 1].is_ascii_digit();
                            if !numeric_comma {
                                clause_commas.push(i);
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            let low_b = body_for_parse.to_ascii_lowercase();
            let primary_split = if let Some((if_idx, &if_comma)) = clause_commas
                .iter()
                .enumerate()
                .find(|(_, c)| low_b[**c..].starts_with(", if "))
            {
                clause_commas.get(if_idx + 1).copied().unwrap_or(if_comma)
            } else {
                clause_commas
                    .first()
                    .copied()
                    .unwrap_or(body_for_parse.len())
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
            let (mut header, init_body) = split_at_pos(primary_split);
            let mut trig_res = parse_trigger_header(header, vocab);
            let (mut ins, mut fails) = parse_effect_body(init_body, vocab);
            // Scan all other unquoted clause commas: first AFTER primary_split (preserving
            // comma-containing intervening-if conditions and 6+ comma Oxford trigger lists),
            // then BEFORE primary_split (handling `, if ` inside the effect body).
            if trig_res.is_err() || !fails.is_empty() {
                for &pos in clause_commas
                    .iter()
                    .filter(|&&c| c > primary_split)
                    .chain(clause_commas.iter().filter(|&&c| c < primary_split))
                {
                    let (cand_h, cand_b) = split_at_pos(pos);
                    if let Ok(t) = parse_trigger_header(cand_h, vocab) {
                        let (cand_ins, cand_fails) = parse_effect_body(cand_b, vocab);
                        if !cand_ins.is_empty() && (cand_fails.is_empty() || trig_res.is_err()) {
                            header = cand_h;
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
            match trig_res {
                Ok(trigger) => {
                    if find_last_modal_modes_mut(&mut ins).is_some() {
                        modal_target = ModalTarget::LastAbility;
                    }
                    let ab = AbilityAst::Triggered {
                        ability_word,
                        trigger,
                        body: ins,
                    };
                    match para_section {
                        ActiveSection::Base => out.abilities.push(ab),
                        ActiveSection::LevelBand => {
                            if let Some(band) = out.level_bands.last_mut() {
                                band.abilities.push(ab);
                            }
                        }
                        ActiveSection::ClassLevel => {
                            if let Some(cl) = out.class_levels.last_mut() {
                                cl.abilities.push(ab);
                            }
                        }
                        ActiveSection::CaseSolved => out.case_solved.abilities.push(ab),
                    }
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
                let (mut ins, fails) = parse_effect_body(eff_str, vocab);
                match parse_cost_list(cost_str, vocab) {
                    Ok(cost) => {
                        if find_last_modal_modes_mut(&mut ins).is_some() {
                            modal_target = ModalTarget::LastAbility;
                        }
                        let ab = AbilityAst::Activated {
                            ability_word,
                            zone: None,
                            cost,
                            body: ins,
                        };
                        match para_section {
                            ActiveSection::Base => out.abilities.push(ab),
                            ActiveSection::LevelBand => {
                                if let Some(band) = out.level_bands.last_mut() {
                                    band.abilities.push(ab);
                                }
                            }
                            ActiveSection::ClassLevel => {
                                if let Some(cl) = out.class_levels.last_mut() {
                                    cl.abilities.push(ab);
                                }
                            }
                            ActiveSection::CaseSolved => out.case_solved.abilities.push(ab),
                        }
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
                let (mut ins, fails) = parse_effect_body(eff_str, vocab);
                if find_last_modal_modes_mut(&mut ins).is_some() {
                    modal_target = ModalTarget::LastAbility;
                }
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
                || low_b.starts_with("reveal ~ as you draft it")
                || low_b.starts_with("draft ~ face up")
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
            let (mut ins, fails) = parse_effect_body(body_for_parse, vocab);
            if find_last_modal_modes_mut(&mut ins).is_some() {
                modal_target = ModalTarget::Spell;
            }
            out.spell_effects.extend(ins);
            out.failures.extend(fails);
            continue;
        }

        // Attraction Visit and Prize paragraphs ALWAYS compile as triggered abilities (never permanent statics).
        if is_attraction
            && ability_word
                .as_deref()
                .is_some_and(|w| w.eq_ignore_ascii_case("visit") || w.eq_ignore_ascii_case("prize"))
        {
            let is_prize = ability_word
                .as_deref()
                .is_some_and(|w| w.eq_ignore_ascii_case("prize"));
            let (mut ins, fails) = parse_effect_body(body_for_parse, vocab);
            if fails.is_empty() && !ins.is_empty() {
                if find_last_modal_modes_mut(&mut ins).is_some() {
                    modal_target = ModalTarget::LastAbility;
                }
                out.abilities.push(AbilityAst::Triggered {
                    ability_word: Some(if is_prize { "Prize" } else { "Visit" }.to_string()),
                    trigger: ast::TriggerSpecAst {
                        events: vec![if is_prize {
                            TriggerEventAst::PrizeClaimedSelf
                        } else {
                            TriggerEventAst::RollDie {
                                player: ast::PlayerRefAst::You,
                                exact_result: None,
                            }
                        }],
                        intervening_if: None,
                    },
                    body: ins,
                });
                continue;
            }
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
            Ok(StaticOrReplacement::Static(st)) => match para_section {
                ActiveSection::Base => out.statics.push(st),
                ActiveSection::LevelBand => {
                    if let Some(band) = out.level_bands.last_mut() {
                        band.statics.push(st);
                    }
                }
                ActiveSection::ClassLevel => {
                    if let Some(cl) = out.class_levels.last_mut() {
                        cl.statics.push(st);
                    }
                }
                ActiveSection::CaseSolved => out.case_solved.statics.push(st),
            },
            Ok(StaticOrReplacement::Replacement(mut rp)) => {
                if promote_as_enters_modal(&mut rp, vocab) {
                    modal_target = ModalTarget::LastReplacement;
                } else if matches!(rp, ast::ReplacementEffectAst::AsEntersChoose { .. }) {
                    last_replacement_is_named_choice = true;
                }
                match para_section {
                    ActiveSection::Base => out.replacements.push(rp),
                    ActiveSection::LevelBand => {
                        if let Some(band) = out.level_bands.last_mut() {
                            band.replacements.push(rp);
                        }
                    }
                    ActiveSection::ClassLevel => {
                        if let Some(cl) = out.class_levels.last_mut() {
                            cl.replacements.push(rp);
                        }
                    }
                    ActiveSection::CaseSolved => out.case_solved.replacements.push(rp),
                }
            }
            Err(reason) => {
                // Multi-sentence static/replacement paragraph (e.g. "~ enters tapped. As it enters, choose a color.")
                let sents = split_sentences(body_for_parse);
                let mut multi_ok = sents.len() >= 2;
                let mut last_was_modal_repl = false;
                let mut sub_kws = Vec::new();
                let mut sub_statics = Vec::new();
                let mut sub_repls = Vec::new();
                if multi_ok {
                    for sent in &sents {
                        last_was_modal_repl = false;
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
                            Ok(StaticOrReplacement::Replacement(mut rp)) => {
                                last_was_modal_repl = promote_as_enters_modal(&mut rp, vocab);
                                sub_repls.push(rp);
                            }
                            Err(_) => {
                                // Allow paired triggered sentences inside multi-sentence static paragraphs
                                // (Keranos, God-Eternal Kefnet, etc.).
                                let trimmed_sent = sent.trim();
                                if (!sub_statics.is_empty() || !sub_repls.is_empty())
                                    && (trimmed_sent.starts_with("Whenever ")
                                        || trimmed_sent.starts_with("When ")
                                        || trimmed_sent.starts_with("At the beginning of "))
                                    && let Some((hdr, body_s)) = trimmed_sent.split_once(", ")
                                    && let Ok(trig) = parse_trigger_header(hdr, vocab)
                                {
                                    let (ins, fails) = parse_effect_body(body_s, vocab);
                                    if fails.is_empty()
                                        && !ins.is_empty()
                                        && (options.allow_tabletop_honor_system
                                            || !ins.iter().any(instruction_uses_honor_system))
                                    {
                                        sub_statics.push(StaticAbilityAst::PairedTrigger {
                                            trigger: trig,
                                            body: ins,
                                        });
                                        continue;
                                    }
                                }
                                // Allow imperative follow-up sentences when at least one static/replacement
                                // has already parsed in this paragraph.
                                let sl = trimmed_sent.to_ascii_lowercase();
                                let is_followup_lead = sl.starts_with("if you do, ")
                                    || sl.starts_with("if you don't, ")
                                    || sl.starts_with("if you can't, ")
                                    || sl.starts_with("if you cast ")
                                    || sl.starts_with("when you do, ")
                                    || sl.starts_with("if x is ")
                                    || sl.starts_with("if it ")
                                    || sl.starts_with("if the ")
                                    || sl.starts_with("if a ")
                                    || sl.starts_with("if that ")
                                    || sl.starts_with("otherwise, ")
                                    || sl.starts_with("until ")
                                    || sl.starts_with("for each ")
                                    || sl.starts_with("you choose ")
                                    || sl.starts_with("you may ")
                                    || sl.starts_with("that copy ")
                                    || sl.starts_with("its base ")
                                    || sl.starts_with("it enters ")
                                    || sl.starts_with("it's still ")
                                    || sl.starts_with("you may activate each of those abilities only once each turn")
                                    || sl.starts_with("do this only once each turn")
                                    || sl.starts_with("the same is true for ")
                                    || sl.starts_with("no more than one mana of each color ")
                                    || sl.starts_with("then ");
                                if (!sub_statics.is_empty() || !sub_repls.is_empty())
                                    && (sl.starts_with("it's still ")
                                        || sl.starts_with("you may activate each of those abilities only once each turn")
                                        || sl.starts_with("do this only once each turn")
                                        || sl.starts_with("the same is true for ")
                                        || sl.starts_with("no more than one mana of each color "))
                                {
                                    sub_statics.push(StaticAbilityAst::RuleOrCombatRestriction {
                                        subject: None,
                                        rule: trimmed_sent.to_string(),
                                        filter: None,
                                        condition: None,
                                    });
                                    continue;
                                }
                                if (!sub_statics.is_empty() || !sub_repls.is_empty())
                                    && is_followup_lead
                                {
                                    let (ins, fails) = parse_effect_body(trimmed_sent, vocab);
                                    if fails.is_empty()
                                        && !ins.is_empty()
                                        && (options.allow_tabletop_honor_system
                                            || !ins.iter().any(instruction_uses_honor_system))
                                    {
                                        sub_statics.push(StaticAbilityAst::FollowUp(ins));
                                        continue;
                                    }
                                }
                                // Ignore-effect escape clause attached to preceding static
                                // ("Any player / Its controller / That player may <cost> for that player to ignore this effect until end of turn.")
                                if let Some(last_st) = sub_statics.last_mut()
                                    && let Some((who_cost, _)) = sl.split_once(
                                        " for that player to ignore this effect until end of turn",
                                    )
                                    && let Some((who_raw, cost_raw)) = who_cost.split_once(" may ")
                                    && let Ok(cost) =
                                        clauses::parse_cost_list(cost_raw.trim(), vocab)
                                {
                                    let prev = std::mem::replace(
                                        last_st,
                                        StaticAbilityAst::DraftAction(String::new()),
                                    );
                                    *last_st = StaticAbilityAst::WithIgnoreCost {
                                        inner: Box::new(prev),
                                        who: who_raw.trim().to_string(),
                                        cost,
                                        duration: ast::DurationAst::UntilEndOfTurn,
                                    };
                                    continue;
                                }
                                // Clarification sentence ("This effect doesn't remove Auras already attached to those artifacts.")
                                if !sub_statics.is_empty() && sl.starts_with("this effect doesn't ")
                                {
                                    sub_statics.push(StaticAbilityAst::RuleOrCombatRestriction {
                                        subject: None,
                                        rule: trimmed_sent.to_string(),
                                        filter: None,
                                        condition: None,
                                    });
                                    continue;
                                }
                                multi_ok = false;
                                break;
                            }
                        }
                    }
                }
                if multi_ok {
                    if last_was_modal_repl {
                        modal_target = ModalTarget::LastReplacement;
                    }
                    match para_section {
                        ActiveSection::Base => {
                            out.keywords.extend(sub_kws);
                            out.statics.extend(sub_statics);
                            out.replacements.extend(sub_repls);
                        }
                        ActiveSection::LevelBand => {
                            if let Some(band) = out.level_bands.last_mut() {
                                band.keywords.extend(sub_kws);
                                band.statics.extend(sub_statics);
                                band.replacements.extend(sub_repls);
                            }
                        }
                        ActiveSection::ClassLevel => {
                            if let Some(cl) = out.class_levels.last_mut() {
                                cl.keywords.extend(sub_kws);
                                cl.statics.extend(sub_statics);
                                cl.replacements.extend(sub_repls);
                            }
                        }
                        ActiveSection::CaseSolved => {
                            out.case_solved.keywords.extend(sub_kws);
                            out.case_solved.statics.extend(sub_statics);
                            out.case_solved.replacements.extend(sub_repls);
                        }
                    }
                    continue;
                }

                // Attraction Visit and Prize effect paragraphs (non-spell permanent with Visit/Prize abilities)
                if is_attraction {
                    let is_prize = ability_word
                        .as_deref()
                        .is_some_and(|w| w.eq_ignore_ascii_case("prize"));
                    let (mut ins, fails) = parse_effect_body(body_for_parse, vocab);
                    if fails.is_empty() && !ins.is_empty() {
                        if find_last_modal_modes_mut(&mut ins).is_some() {
                            modal_target = ModalTarget::LastAbility;
                        }
                        out.abilities.push(AbilityAst::Triggered {
                            ability_word: Some(
                                if is_prize { "Prize" } else { "Visit" }.to_string(),
                            ),
                            trigger: ast::TriggerSpecAst {
                                events: vec![if is_prize {
                                    TriggerEventAst::PrizeClaimedSelf
                                } else {
                                    TriggerEventAst::RollDie {
                                        player: ast::PlayerRefAst::You,
                                        exact_result: None,
                                    }
                                }],
                                intervening_if: None,
                            },
                            body: ins,
                        });
                        continue;
                    }
                    if ability_word.as_deref().is_some_and(|w| {
                        w.eq_ignore_ascii_case("visit") || w.eq_ignore_ascii_case("prize")
                    }) && !fails.is_empty()
                    {
                        out.failures.extend(fails);
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

    fn normalize_standalone_choose_one(ins: &mut [InstructionAst]) {
        for op in ins.iter_mut() {
            match op {
                InstructionAst::Modal { choice, modes } if modes.is_empty() && choice == "one" => {
                    *op = InstructionAst::ChooseFromGroup {
                        count: Box::new(ast::ValueExprAst::Const(1)),
                        random: false,
                        group: Box::new(ast::SubjectRefAst::Anaphora("one of them".to_string())),
                    };
                }
                InstructionAst::Optional(inner)
                | InstructionAst::OptionalPlayer { body: inner, .. }
                | InstructionAst::IfCondition {
                    then_body: inner, ..
                }
                | InstructionAst::IfPreviousSucceeded(inner)
                | InstructionAst::IfPreviousFailed(inner)
                | InstructionAst::Otherwise(inner)
                | InstructionAst::ForEachPlayer { body: inner, .. }
                | InstructionAst::ForEachObject { body: inner, .. }
                | InstructionAst::ForEachCount { body: inner, .. }
                | InstructionAst::DelayedTrigger { body: inner, .. }
                | InstructionAst::DelayedEventTrigger { body: inner, .. }
                | InstructionAst::ReflexiveTrigger(inner)
                | InstructionAst::ScopedDuration { body: inner, .. }
                | InstructionAst::ScopedDurationText { body: inner, .. } => {
                    normalize_standalone_choose_one(inner);
                }
                _ => {}
            }
        }
    }
    normalize_standalone_choose_one(&mut out.spell_effects);
    for ab in &mut out.abilities {
        match ab {
            AbilityAst::Activated { body, .. }
            | AbilityAst::Triggered { body, .. }
            | AbilityAst::Loyalty { body, .. }
            | AbilityAst::SagaChapter { body, .. } => normalize_standalone_choose_one(body),
            AbilityAst::ClassLevel { .. } => {}
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
        StaticAbilityAst::Otherwise(inner)
        | StaticAbilityAst::OncePerTurn(inner)
        | StaticAbilityAst::ScaledBy { inner, .. } => static_uses_honor_system(inner),
        StaticAbilityAst::Compound(items) => items.iter().any(static_uses_honor_system),
        StaticAbilityAst::FollowUp(body) => body.iter().any(instruction_uses_honor_system),
        _ => false,
    }
}

fn section_uses_honor_system(
    statics: &[StaticAbilityAst],
    replacements: &[ast::ReplacementEffectAst],
    abilities: &[AbilityAst],
) -> bool {
    if statics.iter().any(static_uses_honor_system) {
        return true;
    }
    fn replacement_uses_honor_system(r: &ast::ReplacementEffectAst) -> bool {
        match r {
            ast::ReplacementEffectAst::EventInstead {
                instead_body: Some(body),
                ..
            }
            | ast::ReplacementEffectAst::AsEntersAction(body) => {
                body.iter().any(instruction_uses_honor_system)
            }
            ast::ReplacementEffectAst::Compound(items) => {
                items.iter().any(replacement_uses_honor_system)
            }
            ast::ReplacementEffectAst::ScaledBy { inner, .. } => {
                replacement_uses_honor_system(inner)
            }
            _ => false,
        }
    }
    if replacements.iter().any(replacement_uses_honor_system) {
        return true;
    }
    for ab in abilities {
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

fn face_uses_honor_system(face: &FaceAst) -> bool {
    if face.spell_effects.iter().any(instruction_uses_honor_system) {
        return true;
    }
    if section_uses_honor_system(&face.statics, &face.replacements, &face.abilities) {
        return true;
    }
    if face
        .level_bands
        .iter()
        .any(|b| section_uses_honor_system(&b.statics, &b.replacements, &b.abilities))
    {
        return true;
    }
    if face.class_levels.iter().any(|c| {
        c.cost
            .iter()
            .any(|a| matches!(a, ast::CostAtomAst::ManualTabletopAction { .. }))
            || section_uses_honor_system(&c.statics, &c.replacements, &c.abilities)
    }) {
        return true;
    }
    section_uses_honor_system(
        &face.case_solved.statics,
        &face.case_solved.replacements,
        &face.case_solved.abilities,
    )
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
        InstructionAst::Modal { modes, .. } => modes.iter().any(|m| {
            m.body.iter().any(instruction_uses_honor_system)
                || m.granted.as_deref().is_some_and(face_uses_honor_system)
        }),
        InstructionAst::ForEachSubjectPlayer { body, .. }
        | InstructionAst::TemporarySpecialAction { body, .. }
        | InstructionAst::ForEachColorAmong { body, .. } => {
            body.iter().any(instruction_uses_honor_system)
        }
        InstructionAst::CreateEmblemFor {
            compiled: Some(inner),
            ..
        } => face_uses_honor_system(inner),
        _ => false,
    }
}
