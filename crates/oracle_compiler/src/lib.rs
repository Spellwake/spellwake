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
pub mod wave_e6;
pub mod wave_e7;

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

/// Recursively compile a quoted granted ability string into a typed `FaceAst` fragment.
/// Returns `Some` only when the quoted ability compiles cleanly with zero failures.
#[must_use]
pub fn compile_granted_ability(ability_text: &str, vocab: &Vocabulary) -> Option<Box<FaceAst>> {
    let trimmed = ability_text.trim().trim_end_matches(',');
    if trimmed.is_empty() {
        return None;
    }
    let normalized = normalize_nested_single_quotes(trimmed);
    let face = compile_face_with_options(
        FaceInput {
            name: "",
            type_line: "Creature",
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
            out.statics.push(StaticAbilityAst::StickerOption {
                ticket_cost,
                payload,
            });
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

        // Modal bullet line (or bulleted die-roll band such as "• 2 — menace")
        if let Some(bullet_body) = norm.strip_prefix(BULLET) {
            if modal_target == ModalTarget::None
                && let Some((roll, payload)) = parse_die_roll_band(bullet_body.trim())
            {
                let band = StaticAbilityAst::DieRollBand { roll, payload };
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
            let (ins, mut fails) = parse_effect_body(bullet_clean, vocab);
            let mut granted_face: Option<Box<FaceAst>> = None;
            if !fails.is_empty()
                && modal_target == ModalTarget::LastReplacement
                && let Some(compiled) = compile_granted_ability(bullet_clean, vocab)
            {
                fails.clear();
                granted_face = Some(compiled);
            }
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
                out.statics.push(StaticAbilityAst::StationBand {
                    min_charge,
                    payload: right.trim().to_string(),
                });
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
                out.statics.push(StaticAbilityAst::DieRollBand {
                    roll: lt.to_string(),
                    payload: right.trim().to_string(),
                });
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
            out.statics.push(StaticAbilityAst::DieRollBand {
                roll: left.trim().to_string(),
                payload: right.trim().to_string(),
            });
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
        if let Some((kws, k_statics)) = try_parse_keyword_line(body_for_parse, vocab) {
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

                // Attraction Visit effect paragraph (non-spell permanent with Visit die-roll effect)
                if is_attraction {
                    let (mut ins, fails) = parse_effect_body(body_for_parse, vocab);
                    if fails.is_empty() && !ins.is_empty() {
                        if find_last_modal_modes_mut(&mut ins).is_some() {
                            modal_target = ModalTarget::LastAbility;
                        }
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
                    if ability_word
                        .as_deref()
                        .is_some_and(|w| w.eq_ignore_ascii_case("visit"))
                        && !fails.is_empty()
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
    fn stage_a_and_b_lossless_parser_regressions() {
        use crate::ast::{
            DelayedStepScope, ManaSpecAst, StaticAbilityAst, SubjectRefAst, TriggerEventAst,
            TurnStepAst, ValueExprAst, ZoneFilter,
        };
        let vocab = test_vocab();

        // 1. Lossless ZoneFilter variants (never collapse multi-zone/command/outside/their-hand)
        let f_gy_ex = crate::filter::parse_object_filter(
            "a creature card from your graveyard or exile",
            &vocab,
        )
        .expect("gy or exile");
        assert_eq!(f_gy_ex.zone, Some(ZoneFilter::YourGraveyardOrExile));

        let f_hand_gy = crate::filter::parse_object_filter(
            "a creature card from your hand or graveyard",
            &vocab,
        )
        .expect("hand or gy");
        assert_eq!(f_hand_gy.zone, Some(ZoneFilter::YourHandOrGraveyard));

        let f_cmd = crate::filter::parse_object_filter("a card from the command zone", &vocab)
            .expect("cmd");
        assert_eq!(f_cmd.zone, Some(ZoneFilter::CommandZone));

        let f_out =
            crate::filter::parse_object_filter("a card you own from outside the game", &vocab)
                .expect("outside");
        assert_eq!(f_out.zone, Some(ZoneFilter::OutsideTheGame));

        let f_their =
            crate::filter::parse_object_filter("a card from their hand", &vocab).expect("their");
        assert_eq!(f_their.zone, Some(ZoneFilter::ThatPlayerHand));

        // 2. Exotic Orchard & Gond Gate AnyLandCouldProduce ordering
        let exotic = compile_card(
            &CardInput {
                oracle_id: "exotic-orchard",
                name: "Exotic Orchard",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Exotic Orchard",
                    type_line: "Land",
                    oracle_text: "{T}: Add one mana of any color that a land an opponent controls could produce.",
                }],
            },
            &vocab,
        );
        assert!(exotic.strictly_compiled, "{:?}", exotic.faces[0].failures);
        let AbilityAst::Activated { body, .. } = &exotic.faces[0].abilities[0] else {
            panic!("expected activated ability");
        };
        assert!(matches!(
            &body[0],
            InstructionAst::AddMana {
                mana: ManaSpecAst::AnyLandCouldProduce {
                    controller: crate::ast::ControllerRelation::Opponent,
                },
                ..
            }
        ));

        // Gond Gate and Mox Amber compile into exact lossless ManaSpecAst variants
        let gond = compile_card(
            &CardInput {
                oracle_id: "gond-gate",
                name: "Gond Gate",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Gond Gate",
                    type_line: "Land \u{2014} Gate",
                    oracle_text: "{T}: Add one mana of any color that a Gate you control could produce.",
                }],
            },
            &vocab,
        );
        assert!(gond.strictly_compiled, "{:?}", gond.faces[0].failures);
        let AbilityAst::Activated {
            body: gond_body, ..
        } = &gond.faces[0].abilities[0]
        else {
            panic!("expected activated ability on Gond Gate");
        };
        assert!(matches!(
            &gond_body[0],
            InstructionAst::AddMana {
                mana: ManaSpecAst::AnyObjectCouldProduce { filter, any_type: false },
                ..
            } if filter.subtypes.iter().any(|s| s.eq_ignore_ascii_case("gate"))
        ));
        let mox_amber = compile_card(
            &CardInput {
                oracle_id: "mox-amber",
                name: "Mox Amber",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Mox Amber",
                    type_line: "Legendary Artifact",
                    oracle_text: "{T}: Add one mana of any color among legendary creatures and planeswalkers you control.",
                }],
            },
            &vocab,
        );
        assert!(
            mox_amber.strictly_compiled,
            "{:?}",
            mox_amber.faces[0].failures
        );
        let AbilityAst::Activated { body: mox_body, .. } = &mox_amber.faces[0].abilities[0] else {
            panic!("expected activated ability on Mox Amber");
        };
        assert!(matches!(
            &mox_body[0],
            InstructionAst::AddMana {
                mana: ManaSpecAst::AnyColorAmong { .. },
                ..
            }
        ));

        // 3. DieRollBand vs StationBand
        let portent = compile_card(
            &CardInput {
                oracle_id: "die-roll",
                name: "Die Roll Card",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Die Roll Card",
                    type_line: "Instant",
                    oracle_text: "10+ | Draw two cards.",
                }],
            },
            &vocab,
        );
        assert!(matches!(
            portent.faces[0].statics.first(),
            Some(StaticAbilityAst::DieRollBand { .. })
        ));

        // 4. "when ~ leaves the battlefield" emits DelayedEventTrigger, NOT DelayedTrigger(EndStep)
        let ltb = compile_card(
            &CardInput {
                oracle_id: "ltb-test",
                name: "Test Relic",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Test Relic",
                    type_line: "Artifact",
                    oracle_text: "When Test Relic enters, sacrifice it when Test Relic leaves the battlefield.",
                }],
            },
            &vocab,
        );
        let AbilityAst::Triggered { body: ltb_body, .. } = &ltb.faces[0].abilities[0] else {
            panic!("expected trigger");
        };
        assert!(matches!(
            &ltb_body[0],
            InstructionAst::DelayedEventTrigger { trigger, .. }
                if matches!(
                    trigger.events.first(),
                    Some(TriggerEventAst::ZoneChange {
                        from: Some(f),
                        to: None,
                        subject,
                        ..
                    }) if f == "battlefield" && matches!(subject.as_ref(), SubjectRefAst::SelfObject)
                )
        ));

        // 5. DelayedStepScope distinguishes YourNext vs Next vs ThatPlayersNext
        let your_end = compile_card(
            &CardInput {
                oracle_id: "your-end",
                name: "Your End",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Your End",
                    type_line: "Instant",
                    oracle_text: "At the beginning of your next end step, draw a card.",
                }],
            },
            &vocab,
        );
        assert!(matches!(
            your_end.faces[0].spell_effects.first(),
            Some(InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                scope: DelayedStepScope::YourNext,
                ..
            })
        ));
        let next_end = compile_card(
            &CardInput {
                oracle_id: "next-end",
                name: "Next End",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Next End",
                    type_line: "Instant",
                    oracle_text: "At the beginning of the next end step, draw a card.",
                }],
            },
            &vocab,
        );
        assert!(matches!(
            next_end.faces[0].spell_effects.first(),
            Some(InstructionAst::DelayedTrigger {
                step: TurnStepAst::EndStep,
                scope: DelayedStepScope::Next,
                ..
            })
        ));

        // 6. Sacrifice preserves explicit count ("Sacrifice two creatures")
        let sac_two = compile_card(
            &CardInput {
                oracle_id: "sac-two",
                name: "Sac Two",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Sac Two",
                    type_line: "Sorcery",
                    oracle_text: "Target player sacrifices two creatures.",
                }],
            },
            &vocab,
        );
        assert!(sac_two.strictly_compiled, "{:?}", sac_two.faces[0].failures);
        assert!(matches!(
            sac_two.faces[0].spell_effects.first(),
            Some(InstructionAst::Sacrifice {
                count: Some(c),
                ..
            }) if matches!(c.as_ref(), ValueExprAst::Const(2))
        ));

        // 7. Previously-lossy forms now compile into exact lossless typed AST nodes
        let sac_all = compile_card(
            &CardInput {
                oracle_id: "sac-all",
                name: "Lossy Card",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Lossy Card",
                    type_line: "Artifact",
                    oracle_text: "Sacrifice all creatures: Draw a card.",
                }],
            },
            &vocab,
        );
        assert!(sac_all.strictly_compiled, "{:?}", sac_all.faces[0].failures);
        assert!(matches!(
            sac_all.faces[0].abilities.first(),
            Some(AbilityAst::Activated { cost, .. })
                if matches!(cost.as_slice(), [ast::CostAtomAst::SacrificeAll(_)])
        ));

        let sac_compound = compile_card(
            &CardInput {
                oracle_id: "sac-compound",
                name: "Lossy Card",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Lossy Card",
                    type_line: "Artifact",
                    oracle_text: "Sacrifice a creature and Lossy Card: Draw a card.",
                }],
            },
            &vocab,
        );
        assert!(
            sac_compound.strictly_compiled,
            "{:?}",
            sac_compound.faces[0].failures
        );
        assert!(matches!(
            sac_compound.faces[0].abilities.first(),
            Some(AbilityAst::Activated { cost, .. })
                if matches!(
                    cost.as_slice(),
                    [ast::CostAtomAst::Sacrifice { .. }, ast::CostAtomAst::SacrificeSelf]
                )
        ));

        let destroy_except = compile_card(
            &CardInput {
                oracle_id: "destroy-except",
                name: "Lossy Card",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Lossy Card",
                    type_line: "Sorcery",
                    oracle_text: "Destroy all creatures except for Lossy Card.",
                }],
            },
            &vocab,
        );
        assert!(
            destroy_except.strictly_compiled,
            "{:?}",
            destroy_except.faces[0].failures
        );
        assert!(matches!(
            destroy_except.faces[0].spell_effects.first(),
            Some(InstructionAst::Destroy {
                except: Some(ex),
                random: false,
                ..
            }) if matches!(ex.as_ref(), SubjectRefAst::SelfObject)
        ));

        let draw_upto = compile_card(
            &CardInput {
                oracle_id: "draw-upto",
                name: "Lossy Card",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Lossy Card",
                    type_line: "Sorcery",
                    oracle_text: "Draw up to three cards.",
                }],
            },
            &vocab,
        );
        assert!(
            draw_upto.strictly_compiled,
            "{:?}",
            draw_upto.faces[0].failures
        );
        assert!(matches!(
            draw_upto.faces[0].spell_effects.first(),
            Some(InstructionAst::Draw {
                up_to: true,
                count,
                ..
            }) if matches!(count.as_ref(), ValueExprAst::Const(3))
        ));

        // 8. Lossless SearchLibrary with NameFilterAst preserves exact card name
        let named_search = compile_card(
            &CardInput {
                oracle_id: "named-search",
                name: "Named Tutor",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Named Tutor",
                    type_line: "Sorcery",
                    oracle_text: "Search your library for a card named Sol Ring, reveal it, put it into your hand, then shuffle.",
                }],
            },
            &vocab,
        );
        assert!(
            named_search.strictly_compiled,
            "{:?}",
            named_search.faces[0].failures
        );
        assert!(matches!(
            named_search.faces[0].spell_effects.first(),
            Some(InstructionAst::SearchLibrary { filter, .. })
                if filter.named == Some(crate::ast::NameFilterAst::Exact("sol ring".to_string()))
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

    #[test]
    fn pass_e_api_safe_lib_and_preprocess_unlocks() {
        let vocab = test_vocab();

        // 1. Bulleted die-roll rows emit StaticAbilityAst::DieRollBand
        let die_card = compile_card(
            &CardInput {
                oracle_id: "die-bullets",
                name: "Roller Bot",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Roller Bot",
                    type_line: "Artifact Creature \u{2014} Robot",
                    oracle_text: "\u{2022} 2 \u{2014} menace\n\u{2022} 1-9 \u{2014} Draw a card.\n\u{2022} 10+ | Draw two cards.",
                }],
            },
            &vocab,
        );
        assert!(
            die_card.strictly_compiled,
            "{:?}",
            die_card.faces[0].failures
        );
        assert_eq!(die_card.faces[0].statics.len(), 3);
        assert!(
            die_card.faces[0]
                .statics
                .iter()
                .all(|s| matches!(s, StaticAbilityAst::DieRollBand { .. }))
        );

        // 2. Attraction Visit modal bullets attach onto Visit triggered ability
        let attr = compile_card(
            &CardInput {
                oracle_id: "attr-modal",
                name: "Modal Midway",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Modal Midway",
                    type_line: "Artifact \u{2014} Attraction",
                    oracle_text: "Visit \u{2014} Choose one \u{2014}\n\u{2022} Draw a card.\n\u{2022} You gain 2 life.",
                }],
            },
            &vocab,
        );
        assert!(attr.strictly_compiled, "{:?}", attr.faces[0].failures);
        assert!(matches!(
            attr.faces[0].abilities.first(),
            Some(AbilityAst::Triggered { body, .. })
                if matches!(body.first(), Some(InstructionAst::Modal { modes, .. }) if modes.len() == 2)
        ));

        // 3. As ~ enters, choose one — promotes to AsEntersAction modal
        let enters_modal = compile_card(
            &CardInput {
                oracle_id: "enters-modal",
                name: "Modal Idol",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Modal Idol",
                    type_line: "Artifact",
                    oracle_text: "As Modal Idol enters, choose one \u{2014}\n\u{2022} Draw a card.\n\u{2022} You gain 2 life.",
                }],
            },
            &vocab,
        );
        assert!(
            enters_modal.strictly_compiled,
            "{:?}",
            enters_modal.faces[0].failures
        );
        assert!(matches!(
            enters_modal.faces[0].replacements.first(),
            Some(ast::ReplacementEffectAst::AsEntersAction(body))
                if matches!(body.first(), Some(InstructionAst::Modal { modes, .. }) if modes.len() == 2)
        ));

        // 4. Deep Oxford-comma trigger & trigger whose effect body contains ", if "
        let deep_trig = compile_card(
            &CardInput {
                oracle_id: "deep-trig",
                name: "Oxford Oracle",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Oxford Oracle",
                    type_line: "Enchantment",
                    oracle_text: "Whenever you cast an artifact, enchantment, instant, or sorcery spell, draw a card. Then, if you control a creature, you gain 2 life.",
                }],
            },
            &vocab,
        );
        assert!(
            deep_trig.strictly_compiled,
            "{:?}",
            deep_trig.faces[0].failures
        );

        // 5. Eternal-legal sticker cards do not trip physical tabletop guard
        assert!(!preprocess::is_physical_tabletop_clause(
            "add {r} for each vowel on that sticker."
        ));
        assert!(!preprocess::is_physical_tabletop_clause(
            "put a hat sticker on target creature."
        ));
        assert!(!preprocess::is_physical_tabletop_clause(
            "create a hatchling token."
        ));
        assert!(preprocess::is_physical_tabletop_clause(
            "creatures with a hat in their art"
        ));
        assert!(preprocess::is_physical_tabletop_clause("wearing a hat"));

        // 6. Bare "Level 1" on Class opens level 1 with empty cost; bare "Level 2" stays fail-closed
        let cls = compile_card(
            &CardInput {
                oracle_id: "class-l1",
                name: "Test Class",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Test Class",
                    type_line: "Enchantment \u{2014} Class",
                    oracle_text: "Level 1\nWhen Test Class enters, draw a card.",
                }],
            },
            &vocab,
        );
        assert!(cls.strictly_compiled, "{:?}", cls.faces[0].failures);
        assert_eq!(cls.faces[0].class_levels.len(), 1);
        assert_eq!(cls.faces[0].class_levels[0].level, 1);
        assert!(cls.faces[0].class_levels[0].cost.is_empty());

        let bad_cls = compile_card(
            &CardInput {
                oracle_id: "class-l2",
                name: "Bad Class",
                layout: "normal",
                faces: vec![FaceInput {
                    name: "Bad Class",
                    type_line: "Enchantment \u{2014} Class",
                    oracle_text: "Level 2\nWhen Bad Class enters, draw a card.",
                }],
            },
            &vocab,
        );
        assert!(!bad_cls.strictly_compiled);
    }

    #[test]
    #[ignore]
    fn stage1_corpus_audit() {
        use std::collections::{BTreeMap, HashMap};
        use std::io::{BufRead, BufReader};
        use std::path::Path;
        use std::process::{Command, Stdio};

        #[derive(serde::Deserialize)]
        struct RawFace {
            name: String,
            #[serde(default)]
            type_line: Option<String>,
            #[serde(default)]
            oracle_text: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct RawCard {
            #[serde(default)]
            oracle_id: Option<String>,
            name: String,
            layout: String,
            #[serde(default)]
            type_line: Option<String>,
            #[serde(default)]
            oracle_text: Option<String>,
            #[serde(default)]
            card_faces: Option<Vec<RawFace>>,
            #[serde(default)]
            legalities: BTreeMap<String, String>,
        }

        let vocab_path = std::env::var("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
            .expect("SPELLWAKE_CREATURE_VOCABULARY_INPUT");
        let cat_path =
            std::env::var("SPELLWAKE_CATALOGUE_INPUT").expect("SPELLWAKE_CATALOGUE_INPUT");
        let vocab = Vocabulary::load_from_file(Path::new(&vocab_path)).expect("load vocab");

        let mut child = Command::new("gzip")
            .args(["-dc", &cat_path])
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn gzip");
        let stdout = child.stdout.take().expect("gzip stdout");
        let reader = BufReader::new(stdout);

        const BASE_FORMATS: [&str; 6] = [
            "commander",
            "standard",
            "pioneer",
            "modern",
            "legacy",
            "vintage",
        ];

        let mut total = 0usize;
        let mut strict = 0usize;
        let mut by_bucket: HashMap<String, Vec<String>> = HashMap::new();
        let mut by_stage: HashMap<String, usize> = HashMap::new();
        let mut dump_lines: Vec<String> = Vec::new();

        for line in reader.lines() {
            let line = line.expect("read line");
            if line.trim().is_empty() {
                continue;
            }
            let raw: RawCard = serde_json::from_str(&line).expect("parse card json");
            let eligible = BASE_FORMATS.iter().any(|fmt| {
                matches!(
                    raw.legalities.get(*fmt).map(String::as_str),
                    Some("legal" | "restricted")
                )
            });
            if !eligible {
                continue;
            }
            total += 1;
            let owned_faces: Vec<(String, String, String)> = match raw.card_faces {
                Some(fs) if !fs.is_empty() => fs
                    .into_iter()
                    .map(|f| {
                        (
                            f.name,
                            f.type_line.unwrap_or_default(),
                            f.oracle_text.unwrap_or_default(),
                        )
                    })
                    .collect(),
                _ => vec![(
                    raw.name.clone(),
                    raw.type_line.unwrap_or_default(),
                    raw.oracle_text.unwrap_or_default(),
                )],
            };
            let face_inputs: Vec<FaceInput<'_>> = owned_faces
                .iter()
                .map(|(n, tl, ot)| FaceInput {
                    name: n.as_str(),
                    type_line: tl.as_str(),
                    oracle_text: ot.as_str(),
                })
                .collect();
            let ast = compile_card(
                &CardInput {
                    oracle_id: raw.oracle_id.as_deref().unwrap_or(""),
                    name: &raw.name,
                    layout: &raw.layout,
                    faces: face_inputs,
                },
                &vocab,
            );
            if ast.strictly_compiled {
                strict += 1;
            } else {
                for face in &ast.faces {
                    for fail in &face.failures {
                        *by_stage.entry(fail.stage.clone()).or_insert(0) += 1;
                        let norm_reason = fail
                            .reason
                            .split_once(':')
                            .map_or(fail.reason.as_str(), |(h, _)| h);
                        let words: Vec<&str> = fail.text.split_whitespace().take(3).collect();
                        let prefix = words.join(" ").to_ascii_lowercase();
                        let key = format!("{}:{norm_reason}:[{prefix}]", fail.stage);
                        let entry = by_bucket.entry(key).or_default();
                        if entry.len() < 4 {
                            entry.push(format!("[{}] {}", raw.name, fail.text));
                        } else {
                            entry.push(String::new());
                        }
                        dump_lines.push(format!(
                            "{}\t{}\t{}\t{}",
                            fail.stage, norm_reason, raw.name, fail.text
                        ));
                    }
                }
            }
        }
        let _ = child.wait();

        if let Ok(dump_path) = std::env::var("SPELLWAKE_AST_FAIL_DUMP") {
            let _ = std::fs::write(&dump_path, dump_lines.join("\n"));
        }

        let failed = total.saturating_sub(strict);
        eprintln!(
            "=== FAST STAGE 1 AST AUDIT: {strict} / {total} ({:.2}%), {failed} incomplete ===",
            (strict as f64 / total.max(1) as f64) * 100.0
        );
        let mut stage_sorted: Vec<(String, usize)> = by_stage.into_iter().collect();
        stage_sorted.sort_by_key(|e| std::cmp::Reverse(e.1));
        eprintln!("By stage: {:?}", stage_sorted);

        let mut sorted: Vec<(String, Vec<String>)> = by_bucket.into_iter().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.1.len()));
        for (key, samples) in sorted.iter().take(70) {
            let ex: Vec<&str> = samples
                .iter()
                .filter(|s| !s.is_empty())
                .take(3)
                .map(String::as_str)
                .collect();
            eprintln!("  {:5}  {key}  e.g. {:?}", samples.len(), ex);
        }
    }

    #[test]
    fn pass_e6_bug_fixes_and_lossless_fidelity_traps() {
        let members: Vec<String> = [
            "Dragon", "Troll", "Warrior", "Human", "Hero", "Avatar", "Ally", "Knight", "Rogue",
            "Advisor", "Villain", "Otter", "Wizard", "Frog", "Octopus",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let vocab = Vocabulary::from_creature_members(&members);
        let compile = |name: &str, type_line: &str, oracle_text: &str| {
            compile_card(
                &CardInput {
                    oracle_id: "test",
                    name,
                    layout: "normal",
                    faces: vec![FaceInput {
                        name,
                        type_line,
                        oracle_text,
                    }],
                },
                &vocab,
            )
        };

        // Bug 1: normalize_self earliest delimiter ("Shang-Chi and the Ten Rings" -> "~")
        let shang = compile(
            "Shang-Chi and the Ten Rings",
            "Legendary Creature — Human Warrior Hero",
            "First strike\nWhenever you draw a card, put a +1/+1 counter on Shang-Chi.",
        );
        assert!(
            shang.strictly_compiled,
            "Shang-Chi: {:?}",
            shang.faces[0].failures
        );

        // Bug 2: Oxford-comma trigger headers (Sarkhan's Unsealing, Avatar Aang, River Song)
        let sarkhan = compile(
            "Sarkhan's Unsealing",
            "Enchantment",
            "Whenever you cast a creature spell with power 4, 5, or 6, Sarkhan's Unsealing deals 4 damage to any target.",
        );
        assert!(
            sarkhan.strictly_compiled,
            "Sarkhan's Unsealing: {:?}",
            sarkhan.faces[0].failures
        );

        let aang = compile(
            "Avatar Aang",
            "Legendary Creature — Human Avatar Ally",
            "Flying\nWhenever you waterbend, earthbend, firebend, or airbend, draw a card.",
        );
        assert!(
            aang.strictly_compiled,
            "Avatar Aang: {:?}",
            aang.faces[0].failures
        );

        // Bug 3: Multi-quote + trailing comma + nested single quotes (Terminal Velocity, Old-Growth Troll, Bronzehide Lion)
        let term_vel = compile(
            "Terminal Velocity",
            "Sorcery",
            "You may put an artifact or creature card from your hand onto the battlefield. That permanent gains haste, \"When this permanent leaves the battlefield, it deals damage equal to its mana value to each creature,\" and \"At the beginning of your end step, sacrifice this permanent.\"",
        );
        assert!(
            term_vel.strictly_compiled,
            "Terminal Velocity: {:?}",
            term_vel.faces[0].failures
        );

        let og_troll = compile(
            "Old-Growth Troll",
            "Creature — Troll Warrior",
            "Trample\nWhen this creature dies, if it was a creature, return it to the battlefield. It's an Aura enchantment with enchant Forest you control and \"Enchanted Forest has '{T}: Add {G}{G}' and '{1}, {T}, Sacrifice this land: Create a tapped 4/4 green Troll Warrior creature token with trample.'\"",
        );
        assert!(
            og_troll.strictly_compiled,
            "Old-Growth Troll: {:?}",
            og_troll.faces[0].failures
        );

        // Bug 4: "not named ~" in SearchLibrary (Tiamat) & bare protection/ward (Shay Cormac)
        let tiamat = compile(
            "Tiamat",
            "Legendary Creature — Dragon God",
            "Flying\nWhen Tiamat enters, if you cast it, search your library for up to five Dragon cards not named Tiamat that each have different names, reveal them, put them into your hand, then shuffle.",
        );
        assert!(
            tiamat.strictly_compiled,
            "Tiamat: {:?}",
            tiamat.faces[0].failures
        );

        let shay = compile(
            "Shay Cormac",
            "Legendary Creature — Human Knight Rogue",
            "{1}: Permanents your opponents control lose hexproof, indestructible, protection, shroud, and ward until end of turn.",
        );
        assert!(
            shay.strictly_compiled,
            "Shay Cormac: {:?}",
            shay.faces[0].failures
        );

        // Trap #2: Exclude(referenced), NOT EXCLUDE_SELF (Knowledge Pool)
        let kp = compile(
            "Knowledge Pool",
            "Artifact",
            "Whenever a player casts a spell from their hand, that player exiles it. If the player does, they may cast a spell from among other cards exiled with Knowledge Pool without paying its mana cost.",
        );
        assert!(
            kp.strictly_compiled,
            "Knowledge Pool: {:?}",
            kp.faces[0].failures
        );
        let kp_json = serde_json::to_string(&kp).unwrap_or_default();
        assert!(
            kp_json.contains("\"Exclude\""),
            "Knowledge Pool must use FilterPredicateAst::Exclude, got: {kp_json}"
        );

        // Trap #3: Evil's Thrall (UntilEndOfNextTurn = controller's next turn) vs Nuka-Nuke Launcher (UntilEndOfPlayersNextTurn(DefendingPlayer))
        let thrall = compile(
            "Evil's Thrall",
            "Sorcery",
            "Gain control of target creature until end of turn. If you control a Villain with greater mana value than that creature, gain control of that creature until the end of your next turn instead.",
        );
        assert!(
            thrall.strictly_compiled,
            "Evil's Thrall: {:?}",
            thrall.faces[0].failures
        );
        let thrall_json = serde_json::to_string(&thrall).unwrap_or_default();
        assert!(
            thrall_json.contains("\"UntilEndOfNextTurn\"")
                || thrall_json.contains("\"UntilEndOfPlayersNextTurn\""),
            "Evil's Thrall must preserve end-of-next-turn duration: {thrall_json}"
        );

        let nuka = compile(
            "Nuka-Nuke Launcher",
            "Artifact — Equipment",
            "Whenever equipped creature attacks, until the end of defending player's next turn, that player gets two rad counters whenever they cast a spell.",
        );
        assert!(
            nuka.strictly_compiled,
            "Nuka-Nuke Launcher: {:?}",
            nuka.faces[0].failures
        );
        let nuka_json = serde_json::to_string(&nuka).unwrap_or_default();
        assert!(
            nuka_json.contains("\"UntilEndOfPlayersNextTurn\":\"DefendingPlayer\""),
            "Nuka-Nuke Launcher must preserve defending-player scoped duration: {nuka_json}"
        );

        // Trap #5: Stinging Study (greatest: false) vs Cloudkill (greatest: true)
        let sting = compile(
            "Stinging Study",
            "Instant",
            "You draw X cards and you lose X life, where X is the mana value of a commander you own on the battlefield or in the command zone.",
        );
        assert!(
            sting.strictly_compiled,
            "Stinging Study: {:?}",
            sting.faces[0].failures
        );
        let sting_json = serde_json::to_string(&sting).unwrap_or_default();
        assert!(
            sting_json.contains("\"greatest\":false"),
            "Stinging Study must allow choosing partner commander (greatest: false)"
        );

        let cloudkill = compile(
            "Cloudkill",
            "Sorcery",
            "All creatures get -X/-X until end of turn, where X is the greatest mana value of a commander you own on the battlefield or in the command zone.",
        );
        assert!(
            cloudkill.strictly_compiled,
            "Cloudkill: {:?}",
            cloudkill.faces[0].failures
        );
        let ck_json = serde_json::to_string(&cloudkill).unwrap_or_default();
        assert!(
            ck_json.contains("\"greatest\":true"),
            "Cloudkill must use greatest: true"
        );

        // Trap #6: SeparateIntoPiles face_down_piles (Atris)
        let atris = compile(
            "Atris, Oracle of Half-Truths",
            "Legendary Creature — Human Advisor",
            "Menace\nWhen Atris enters, target opponent looks at the top three cards of your library and separates them into a face-down pile and a face-up pile.",
        );
        assert!(
            atris.strictly_compiled,
            "Atris: {:?}",
            atris.faces[0].failures
        );
        let atris_json = serde_json::to_string(&atris).unwrap_or_default();
        assert!(
            atris_json.contains("\"face_down_piles\":1"),
            "Atris must preserve face_down_piles: 1"
        );

        // Trap #8: The First Tyrannic War must use OthersEnterWithCounters (replacement), never PutCounters
        let tyrannic = compile(
            "The First Tyrannic War",
            "Enchantment — Saga",
            "I — You may put a creature card from your hand onto the battlefield. If its mana cost contains {X}, it enters with a number of +1/+1 counters on it equal to the number of lands you control.",
        );
        assert!(
            tyrannic.strictly_compiled,
            "The First Tyrannic War: {:?}",
            tyrannic.faces[0].failures
        );
        let tyr_json = serde_json::to_string(&tyrannic).unwrap_or_default();
        assert!(
            tyr_json.contains("\"OthersEnterWithCounters\""),
            "0/0 Hydra trap: must emit OthersEnterWithCounters"
        );
        assert!(
            !tyr_json.contains("\"PutCounters\""),
            "0/0 Hydra trap: must not emit PutCounters"
        );

        // Trap #9: Serpentine Spike DistinctTarget + Ballroom Brawlers GrantKeywordChoice
        let spike = compile(
            "Serpentine Spike",
            "Sorcery",
            "Serpentine Spike deals 2 damage to target creature, 3 damage to another target creature, and 4 damage to a third target creature.",
        );
        assert!(
            spike.strictly_compiled,
            "Serpentine Spike: {:?}",
            spike.faces[0].failures
        );
        let spike_json = serde_json::to_string(&spike).unwrap_or_default();
        assert!(
            spike_json.contains("\"DistinctTarget\""),
            "Serpentine Spike must preserve DistinctTarget slots"
        );

        // Trap #10: Turtles Forever exact: true, Master of the Wild Hunt divider: ItsController, Gatecrash Signal the Clans
        let turtles = compile(
            "Turtles Forever",
            "Sorcery",
            "Search your library and/or outside the game for exactly four legendary creature cards you own with different names, then reveal those cards.",
        );
        assert!(
            turtles.strictly_compiled,
            "Turtles Forever: {:?}",
            turtles.faces[0].failures
        );
        let turtles_json = serde_json::to_string(&turtles).unwrap_or_default();
        assert!(
            turtles_json.contains("\"exact\":true"),
            "Turtles Forever must preserve exact: true"
        );

        let signal = compile(
            "Signal the Clans",
            "Instant",
            "Search your library for three creature cards and reveal them. If you reveal three cards with different names, choose one of them at random and put that card into your hand. Shuffle the rest into your library.",
        );
        assert!(
            signal.strictly_compiled,
            "Signal the Clans (gtc): {:?}",
            signal.faces[0].failures
        );
    }

    #[test]
    fn pass_e7_untap_and_lossless_upgrades() {
        let members: Vec<String> = [
            "Shark",
            "Illusion",
            "Drake",
            "Human",
            "Wizard",
            "Goblin",
            "Artificer",
            "Dragon",
            "Avatar",
            "Zombie",
            "God",
            "Ooze",
            "Tiefling",
            "Warrior",
            "Phyrexian",
            "Angel",
            "Wall",
            "Fish",
            "Beast",
            "Bird",
            "Hippo",
            "Construct",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let vocab = Vocabulary::from_creature_members(&members);
        let compile = |name: &str, type_line: &str, oracle_text: &str| {
            compile_card(
                &CardInput {
                    oracle_id: "test",
                    name,
                    layout: "normal",
                    faces: vec![FaceInput {
                        name,
                        type_line,
                        oracle_text,
                    }],
                },
                &vocab,
            )
        };

        // 1. Giant Shark trigger routing: uses typed DEALT_DAMAGE_THIS_TURN instead of dead branch
        let shark = compile(
            "Giant Shark",
            "Creature — Shark",
            "This creature can't attack unless defending player controls an Island.\nWhenever this creature blocks or becomes blocked by a creature that has been dealt damage this turn, this creature gets +2/+0 and gains trample until end of turn.\nWhen you control no Islands, sacrifice this creature.",
        );
        assert!(
            shark.strictly_compiled,
            "Giant Shark: {:?}",
            shark.faces[0].failures
        );
        let has_typed_block_trig = shark.faces[0].abilities.iter().any(|ab| {
            matches!(
                ab,
                AbilityAst::Triggered { trigger, .. }
                    if trigger.events.iter().any(|ev| matches!(
                        ev,
                        TriggerEventAst::BlocksOrBecomesBlockedBy {
                            other_filter,
                            blocks: true,
                            becomes_blocked: true,
                            min_count: 1,
                            ..
                        } if other_filter.flags.0 & ast::ObjectFilterFlags::DEALT_DAMAGE_THIS_TURN.0 != 0
                    ))
            )
        });
        assert!(
            has_typed_block_trig,
            "Giant Shark must emit typed BlocksOrBecomesBlockedBy with DEALT_DAMAGE_THIS_TURN: {:?}",
            shark.faces[0].abilities
        );

        // 2. InstructionAst::UntapPermanent preserves count and up_to for non-targeted untap cards
        let palinchron = compile(
            "Palinchron",
            "Creature — Illusion",
            "Flying\nWhen Palinchron enters, untap up to seven lands.\n{2}{U}{U}: Return Palinchron to its owner's hand.",
        );
        assert!(palinchron.strictly_compiled);
        assert!(palinchron.faces[0].abilities.iter().any(|ab| matches!(
            ab,
            AbilityAst::Triggered { body, .. }
                if body.iter().any(|ins| matches!(
                    ins,
                    InstructionAst::UntapPermanent {
                        subject,
                        count: Some(cnt),
                        up_to: true,
                    } if matches!(subject.as_ref(), ast::SubjectRefAst::Group(f) if f.any_types == ast::CardTypeMask::LAND)
                        && **cnt == ast::ValueExprAst::Const(7)
                ))
        )));

        let frantic = compile(
            "Frantic Search",
            "Instant",
            "Draw two cards, then discard two cards. Untap up to three lands.",
        );
        assert!(frantic.strictly_compiled);
        assert!(frantic.faces[0].spell_effects.iter().any(|ins| matches!(
            ins,
            InstructionAst::UntapPermanent {
                count: Some(cnt),
                up_to: true,
                ..
            } if **cnt == ast::ValueExprAst::Const(3)
        )));

        let azusa = compile(
            "Likeness of the Seeker",
            "Enchantment Creature — Human Monk",
            "When Likeness of the Seeker enters, untap up to three lands you control.",
        );
        assert!(azusa.strictly_compiled);
        assert!(azusa.faces[0].abilities.iter().any(|ab| matches!(
            ab,
            AbilityAst::Triggered { body, .. }
                if body.iter().any(|ins| matches!(
                    ins,
                    InstructionAst::UntapPermanent {
                        subject,
                        count: Some(cnt),
                        up_to: true,
                    } if matches!(subject.as_ref(), ast::SubjectRefAst::Group(f) if f.controller == Some(ast::ControllerRelation::You))
                        && **cnt == ast::ValueExprAst::Const(3)
                ))
        )));

        let shriekwood = compile(
            "Shriekwood Devourer",
            "Creature — Treefolk",
            "Whenever Shriekwood Devourer attacks, untap up to X lands, where X is its power.",
        );
        assert!(shriekwood.strictly_compiled);
        let shriek_json = serde_json::to_string(&shriekwood).unwrap_or_default();
        assert!(
            shriek_json.contains("\"UntapPermanent\"")
                && shriek_json.contains("\"ChosenX\"")
                && shriek_json.contains("\"up_to\":true"),
            "Shriekwood Devourer must preserve ChosenX + up_to: {shriek_json}"
        );

        // Targeted untap preserves TargetObject with count: None, up_to: false
        let twiddle = compile(
            "Garruk Wildspeaker",
            "Planeswalker — Garruk",
            "+1: Untap two target lands.",
        );
        assert!(twiddle.strictly_compiled);
        assert!(twiddle.faces[0].abilities.iter().any(|ab| matches!(
            ab,
            AbilityAst::Loyalty { body, .. }
                if body.iter().any(|ins| matches!(
                    ins,
                    InstructionAst::UntapPermanent {
                        subject,
                        count: None,
                        up_to: false,
                    } if matches!(subject.as_ref(), ast::SubjectRefAst::TargetObject { count: ast::TargetCountAst::Exact(2), .. })
                ))
        )));

        // 3. All 9 high-playability black-border staples compile strictly with zero StructuredRule / GameStateFlag
        let staples = [
            (
                "Chains of Mephistopheles",
                "Enchantment",
                "If a player would draw a card except the first one they draw in each of their draw steps, that player discards a card instead. If the player discards a card this way, they draw a card. If the player doesn't discard a card this way, they mill a card.",
            ),
            (
                "Chancellor of the Annex",
                "Creature — Phyrexian Angel",
                "You may reveal this card from your opening hand. If you do, when each opponent casts their first spell of the game, counter that spell unless that player pays {1}.\nFlying\nWhenever an opponent casts a spell, counter it unless that player pays {1}.",
            ),
            (
                "Goblin Welder",
                "Creature — Goblin Artificer",
                "{T}: Choose target artifact a player controls and target artifact card in that player's graveyard. If both targets are still legal as this ability resolves, that player simultaneously sacrifices the artifact and returns the artifact card to the battlefield.",
            ),
            (
                "Helm of Obedience",
                "Artifact",
                "{X}, {T}: Target opponent mills a card, then repeats this process until a creature card or X cards have been put into their graveyard this way, whichever comes first. If one or more creature cards were put into that graveyard this way, sacrifice this artifact and put one of them onto the battlefield under your control. X can't be 0.",
            ),
            (
                "Niv-Mizzet Reborn",
                "Legendary Creature — Dragon Avatar",
                "Flying\nWhen Niv-Mizzet enters, reveal the top ten cards of your library. For each color pair, choose a card that's exactly those colors from among them. Put the chosen cards into your hand and the rest on the bottom of your library in a random order.",
            ),
            (
                "The Book of Vile Darkness",
                "Legendary Artifact — Book",
                "At the beginning of your end step, if you lost 2 or more life this turn, create a 2/2 black Zombie creature token.\n{T}, Exile The Book of Vile Darkness and artifacts you control named Eye of Vecna and Hand of Vecna: Create Vecna, a legendary 8/8 black Zombie God creature token with indestructible and all triggered abilities of the exiled cards.",
            ),
            (
                "The Chain Veil",
                "Legendary Artifact",
                "At the beginning of your end step, if you didn't activate a loyalty ability of a planeswalker this turn, you lose 2 life.\n{4}, {T}: For each planeswalker you control, you may activate one of its loyalty abilities once this turn as though none of its loyalty abilities have been activated this turn.",
            ),
            (
                "The Mimeoplasm",
                "Legendary Creature — Ooze",
                "As The Mimeoplasm enters, you may exile two creature cards from graveyards. If you do, it enters as a copy of one of those cards with a number of additional +1/+1 counters on it equal to the power of the other card.",
            ),
            (
                "Zevlor, Elturel Exile",
                "Legendary Creature — Tiefling Warrior",
                "Haste\n{2}, {T}: When you next cast an instant or sorcery spell that targets only a single opponent or a single permanent an opponent controls this turn, for each other opponent, choose that player or a permanent they control, copy that spell, and the copy targets the chosen player or permanent.",
            ),
        ];
        for (name, tl, ot) in staples {
            let compiled = compile(name, tl, ot);
            assert!(
                compiled.strictly_compiled,
                "{name} failed: {:?}",
                compiled.faces[0].failures
            );
            let json = serde_json::to_string(&compiled).unwrap_or_default();
            assert!(
                !json.contains("\"StructuredRule\""),
                "{name} must not emit StructuredRule: {json}"
            );
            assert!(
                !json.contains("\"GameStateFlag\""),
                "{name} must not emit GameStateFlag: {json}"
            );
        }

        // Verify typed AST structure on key Pass E7 orthogonal variants
        let welder = compile(
            "Goblin Welder",
            "Creature — Goblin Artificer",
            "{T}: Choose target artifact a player controls and target artifact card in that player's graveyard. If both targets are still legal as this ability resolves, that player simultaneously sacrifices the artifact and returns the artifact card to the battlefield.",
        );
        let welder_json = serde_json::to_string(&welder).unwrap_or_default();
        assert!(
            welder_json.contains("\"AllTargetsLegal\"") && welder_json.contains("\"Simultaneous\""),
            "Goblin Welder must emit AllTargetsLegal + Simultaneous: {welder_json}"
        );

        let snowblind = compile(
            "Snowblind",
            "Enchantment — Aura",
            "Enchant creature\nEnchanted creature gets -X/-Y. If that creature is attacking, X is the number of snow lands defending player controls. Otherwise, X is the number of snow lands its controller controls. Y is equal to X or to enchanted creature's toughness minus 1, whichever is smaller.",
        );
        assert!(
            snowblind.strictly_compiled,
            "Snowblind: {:?}",
            snowblind.faces[0].failures
        );
        let snow_json = serde_json::to_string(&snowblind).unwrap_or_default();
        assert!(
            snow_json.contains("\"DynamicModifyPT\"") && snow_json.contains("\"Conditional\""),
            "Snowblind must emit DynamicModifyPT + Conditional: {snow_json}"
        );

        let wild = compile(
            "Wild Endeavor",
            "Sorcery",
            "Roll two d4 and choose one result. Create a number of 3/3 green Beast creature tokens equal to that result. Then search your library for a number of basic land cards equal to the other result, put them onto the battlefield tapped, then shuffle.",
        );
        assert!(
            wild.strictly_compiled,
            "Wild Endeavor: {:?}",
            wild.faces[0].failures
        );
    }
}
