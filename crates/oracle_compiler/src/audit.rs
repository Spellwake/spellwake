//! Automated Stage 1 Oracle Parser semantic purity and structural invariant auditor.
//!
//! Unlike `CardAst::strictly_compiled` (which checks only `failures.is_empty()`), this module
//! audits compiled Magic card ASTs for silent semantic corruption, Rust `Debug` leaks, bogus
//! keyword hijacks, malformed mana choices, spurious `MODIFIED` flag collisions, honor-system
//! leaks, uncompiled embedded abilities, and opaque English string fallbacks.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ast::{
    AbilityAst, CardAst, FaceAst, InstructionAst, InteractiveChoiceKind, KeywordAst,
    ObjectFilterFlags, ReplacementEffectAst, StaticAbilityAst,
};
use crate::preprocess::{is_mana_sequence, strip_reminder};
use crate::{CardInput, CompileOptions};

/// Severity of an automated parser audit finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuditSeverity {
    /// Confirmed structural corruption, silent data loss, or invariant breach.
    Critical,
    /// Uncompiled embedded rules text or opaque prose fallback that hides semantics from lowering.
    Opaque,
}

/// One machine-verified audit finding on a compiled card AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditFinding {
    pub code: String,
    pub severity: AuditSeverity,
    pub path: String,
    pub detail: String,
}

impl AuditFinding {
    /// True when this finding represents silent structural or semantic AST corruption rather than
    /// an explicit `strict_compile_failure` (for example from a newly released set's mechanics).
    #[must_use]
    pub fn is_structural_defect(&self) -> bool {
        self.severity == AuditSeverity::Critical && self.code != "strict_compile_failure"
    }
}

/// Per-card audit report separating strict parse success, critical defects, and opaque strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardAuditReport {
    pub oracle_id: String,
    pub name: String,
    pub layout: String,
    pub strictly_compiled: bool,
    pub critical_defects: Vec<AuditFinding>,
    pub opaque_strings: Vec<AuditFinding>,
}

impl CardAuditReport {
    /// True when the card compiled strictly with zero critical defects and zero opaque string fallbacks.
    #[must_use]
    pub fn is_fully_typed_pure(&self) -> bool {
        self.strictly_compiled && self.critical_defects.is_empty() && self.opaque_strings.is_empty()
    }

    /// True when the card compiled strictly with zero critical structural/semantic defects.
    #[must_use]
    pub fn has_zero_critical_defects(&self) -> bool {
        self.strictly_compiled && self.critical_defects.is_empty()
    }

    /// True when the card has zero structural/semantic corruption defects (independent of
    /// explicit `strict_compile_failure` findings on unparsed clauses).
    #[must_use]
    pub fn has_zero_structural_defects(&self) -> bool {
        !self
            .critical_defects
            .iter()
            .any(AuditFinding::is_structural_defect)
    }
}

/// Aggregated corpus-wide audit summary for CI gates, artifacts, and README badges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusAuditSummary {
    pub total_cards: usize,
    pub strictly_compiled_cards: usize,
    pub zero_critical_defect_cards: usize,
    pub zero_structural_defect_cards: usize,
    pub fully_typed_pure_cards: usize,
    pub cards_with_critical_defects: usize,
    pub cards_with_structural_defects: usize,
    pub cards_with_strict_failures: usize,
    pub cards_with_opaque_strings: usize,
    pub critical_defect_counts: BTreeMap<String, usize>,
    pub critical_defect_samples: BTreeMap<String, Vec<String>>,
    pub structural_defect_counts: BTreeMap<String, usize>,
    pub structural_defect_samples: BTreeMap<String, Vec<String>>,
    pub opaque_bucket_counts: BTreeMap<String, usize>,
    pub opaque_bucket_samples: BTreeMap<String, Vec<String>>,
}

impl CorpusAuditSummary {
    /// Record one card's audit report into the aggregate summary (capped at 10 sample cards per code).
    pub fn record(&mut self, report: &CardAuditReport) {
        self.total_cards += 1;
        if report.strictly_compiled {
            self.strictly_compiled_cards += 1;
        } else {
            self.cards_with_strict_failures += 1;
        }
        if report.has_zero_critical_defects() {
            self.zero_critical_defect_cards += 1;
        }
        if report.has_zero_structural_defects() {
            self.zero_structural_defect_cards += 1;
        } else {
            self.cards_with_structural_defects += 1;
        }
        if report.is_fully_typed_pure() {
            self.fully_typed_pure_cards += 1;
        }
        if !report.critical_defects.is_empty() {
            self.cards_with_critical_defects += 1;
        }
        if !report.opaque_strings.is_empty() {
            self.cards_with_opaque_strings += 1;
        }
        let mut seen_crit = BTreeSet::new();
        let mut seen_struct = BTreeSet::new();
        for f in &report.critical_defects {
            *self
                .critical_defect_counts
                .entry(f.code.clone())
                .or_insert(0) += 1;
            if seen_crit.insert(f.code.clone()) {
                let samples = self
                    .critical_defect_samples
                    .entry(f.code.clone())
                    .or_default();
                if samples.len() < 10 {
                    samples.push(format!("{} [{}]: {}", report.name, f.path, f.detail));
                }
            }
            if f.is_structural_defect() {
                *self
                    .structural_defect_counts
                    .entry(f.code.clone())
                    .or_insert(0) += 1;
                if seen_struct.insert(f.code.clone()) {
                    let samples = self
                        .structural_defect_samples
                        .entry(f.code.clone())
                        .or_default();
                    if samples.len() < 10 {
                        samples.push(format!("{} [{}]: {}", report.name, f.path, f.detail));
                    }
                }
            }
        }
        let mut seen_op = BTreeSet::new();
        for f in &report.opaque_strings {
            *self.opaque_bucket_counts.entry(f.code.clone()).or_insert(0) += 1;
            if seen_op.insert(f.code.clone()) {
                let samples = self
                    .opaque_bucket_samples
                    .entry(f.code.clone())
                    .or_default();
                if samples.len() < 10 {
                    samples.push(format!("{} [{}]: {}", report.name, f.path, f.detail));
                }
            }
        }
    }
}

/// Audit a single compiled card against its printed input for structural and semantic defects.
#[must_use]
pub fn audit_card(
    input: &CardInput<'_>,
    ast: &CardAst,
    options: CompileOptions,
) -> CardAuditReport {
    let mut critical_defects = Vec::new();
    let mut opaque_strings = Vec::new();

    if !ast.strictly_compiled {
        let before = critical_defects.len();
        for (idx, face) in ast.faces.iter().enumerate() {
            for fail in &face.failures {
                critical_defects.push(AuditFinding {
                    code: "strict_compile_failure".to_string(),
                    severity: AuditSeverity::Critical,
                    path: format!("faces[{idx}].failures"),
                    detail: format!("{}:{}: {}", fail.stage, fail.reason, fail.text),
                });
            }
        }
        if critical_defects.len() == before {
            critical_defects.push(AuditFinding {
                code: "strict_compile_failure".to_string(),
                severity: AuditSeverity::Critical,
                path: "strictly_compiled".to_string(),
                detail: "CardAst::strictly_compiled is false with no recorded face failures"
                    .to_string(),
            });
        }
    }

    for (idx, face) in ast.faces.iter().enumerate() {
        let face_input = input.faces.get(idx);
        let oracle_text = face_input.map_or("", |f| f.oracle_text);
        let type_line = face_input.map_or(face.type_line.as_str(), |f| f.type_line);
        let face_path = format!("faces[{idx}]");
        audit_face(
            face,
            oracle_text,
            type_line,
            &face_path,
            &mut critical_defects,
            &mut opaque_strings,
        );
    }

    if let Ok(json_val) = serde_json::to_value(ast) {
        let face_oracles_low: Vec<String> = input
            .faces
            .iter()
            .map(|f| f.oracle_text.to_ascii_lowercase())
            .collect();
        if !options.allow_tabletop_honor_system {
            let serialized = serde_json::to_string(&json_val).unwrap_or_default();
            if serialized.contains("\"ManualTabletop") {
                critical_defects.push(AuditFinding {
                    code: "honor_system_leak".to_string(),
                    severity: AuditSeverity::Critical,
                    path: "card".to_string(),
                    detail: "ManualTabletop node emitted while allow_tabletop_honor_system=false"
                        .to_string(),
                });
            }
        }
        walk_json_invariants(
            &json_val,
            "",
            None,
            &face_oracles_low,
            &mut critical_defects,
            &mut opaque_strings,
        );
    }

    CardAuditReport {
        oracle_id: ast.oracle_id.clone(),
        name: ast.name.clone(),
        layout: ast.layout.clone(),
        strictly_compiled: ast.strictly_compiled,
        critical_defects,
        opaque_strings,
    }
}

fn audit_face(
    face: &FaceAst,
    oracle_text: &str,
    type_line: &str,
    path: &str,
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    // 1. Empty AST on non-vanilla face (after stripping reminder text).
    let stripped = strip_reminder(oracle_text);
    let has_rules_text = stripped.lines().any(|l| !l.trim().is_empty());
    let ast_items = face.keywords.len()
        + face.abilities.len()
        + face.spell_effects.len()
        + face.statics.len()
        + face.replacements.len()
        + face.level_bands.len()
        + face.class_levels.len()
        + usize::from(
            !face.case_solved.keywords.is_empty()
                || !face.case_solved.statics.is_empty()
                || !face.case_solved.abilities.is_empty()
                || !face.case_solved.replacements.is_empty(),
        );
    // 1. Silent empty AST on non-vanilla face (only when no parse failure was already recorded).
    if has_rules_text && ast_items == 0 && face.failures.is_empty() {
        critical.push(AuditFinding {
            code: "empty_nonvanilla_face".to_string(),
            severity: AuditSeverity::Critical,
            path: path.to_string(),
            detail: format!(
                "non-empty oracle text produced 0 AST items with no recorded failures: {}",
                stripped.trim()
            ),
        });
    }

    // 2. Keyword well-formedness (catch Fluctuator / Catalyst Stone / Fighter Class / Planeswalk hijacks).
    let is_spell = (type_line.contains("Instant") || type_line.contains("Sorcery"))
        && !(type_line.contains("Creature")
            || type_line.contains("Artifact")
            || type_line.contains("Enchantment")
            || type_line.contains("Land")
            || type_line.contains("Planeswalker"));
    audit_face_contents(face, is_spell, path, critical, opaque);
}

fn audit_face_contents(
    face: &FaceAst,
    is_spell: bool,
    path: &str,
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    audit_section_items(
        &face.keywords,
        &face.abilities,
        &face.statics,
        &face.replacements,
        is_spell,
        path,
        critical,
        opaque,
    );
    for (i, ins) in face.spell_effects.iter().enumerate() {
        check_instruction_structure(ins, &format!("{path}.spell_effects[{i}]"), critical, opaque);
    }
    for (i, band) in face.level_bands.iter().enumerate() {
        audit_section_items(
            &band.keywords,
            &band.abilities,
            &band.statics,
            &band.replacements,
            false,
            &format!("{path}.level_bands[{i}]"),
            critical,
            opaque,
        );
    }
    for (i, lvl) in face.class_levels.iter().enumerate() {
        audit_section_items(
            &lvl.keywords,
            &lvl.abilities,
            &lvl.statics,
            &lvl.replacements,
            false,
            &format!("{path}.class_levels[{i}]"),
            critical,
            opaque,
        );
    }
    audit_section_items(
        &face.case_solved.keywords,
        &face.case_solved.abilities,
        &face.case_solved.statics,
        &face.case_solved.replacements,
        false,
        &format!("{path}.case_solved"),
        critical,
        opaque,
    );
}

#[allow(clippy::too_many_arguments)]
fn audit_section_items(
    keywords: &[KeywordAst],
    abilities: &[AbilityAst],
    statics: &[StaticAbilityAst],
    replacements: &[ReplacementEffectAst],
    is_spell: bool,
    path: &str,
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    for (kidx, kw) in keywords.iter().enumerate() {
        check_keyword_well_formed(kw, is_spell, &format!("{path}.keywords[{kidx}]"), critical);
    }
    for (i, ab) in abilities.iter().enumerate() {
        match ab {
            AbilityAst::Activated { body, .. }
            | AbilityAst::Triggered { body, .. }
            | AbilityAst::Loyalty { body, .. }
            | AbilityAst::SagaChapter { body, .. } => {
                for (j, ins) in body.iter().enumerate() {
                    check_instruction_structure(
                        ins,
                        &format!("{path}.abilities[{i}].body[{j}]"),
                        critical,
                        opaque,
                    );
                }
            }
            AbilityAst::ClassLevel { .. } => {}
        }
    }
    for (i, st) in statics.iter().enumerate() {
        check_static_structure(st, &format!("{path}.statics[{i}]"), critical, opaque);
    }
    for (i, rp) in replacements.iter().enumerate() {
        check_replacement_structure(rp, &format!("{path}.replacements[{i}]"), critical, opaque);
    }
}

fn check_replacement_structure(
    rp: &ReplacementEffectAst,
    path: &str,
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    if let ReplacementEffectAst::AsEntersAction(body) = rp {
        for (i, ins) in body.iter().enumerate() {
            check_instruction_structure(ins, &format!("{path}.body[{i}]"), critical, opaque);
        }
    }
}

fn check_keyword_well_formed(
    kw: &KeywordAst,
    is_spell: bool,
    path: &str,
    critical: &mut Vec<AuditFinding>,
) {
    let nlow = kw.name.to_ascii_lowercase();
    if nlow.ends_with("cycling") && nlow.contains(' ') {
        let prefix = nlow.trim_end_matches("cycling").trim();
        if prefix.split_whitespace().count() > 2
            || prefix.contains("has")
            || prefix.contains("pay")
            || prefix.contains("cast")
            || prefix.contains("card")
        {
            critical.push(AuditFinding {
                code: "malformed_keyword_hijack".to_string(),
                severity: AuditSeverity::Critical,
                path: path.to_string(),
                detail: format!("hijacked cycling keyword name: {:?}", kw.name),
            });
        }
    }
    if nlow == "landwalk" && kw.parameter.as_deref() == Some("planes") {
        critical.push(AuditFinding {
            code: "malformed_keyword_hijack".to_string(),
            severity: AuditSeverity::Critical,
            path: path.to_string(),
            detail: "Planeswalk action hijacked as landwalk(planes)".to_string(),
        });
    }
    if is_spell && matches!(nlow.as_str(), "earthbend" | "airbend" | "waterbend") {
        critical.push(AuditFinding {
            code: "malformed_keyword_hijack".to_string(),
            severity: AuditSeverity::Critical,
            path: path.to_string(),
            detail: format!("spell keyword action `{nlow}` hijacked into face.keywords"),
        });
    }
    if let Some(param) = &kw.parameter {
        let plow = param.to_ascii_lowercase();
        if plow.starts_with("costs ")
            || plow.starts_with("cost ")
            || plow.starts_with("abilities ")
            || plow.starts_with("ability ")
            || plow.starts_with("triggers ")
            || plow.contains(" cost {")
            || plow.contains(" costs {")
            || plow.contains(" you activate")
        {
            critical.push(AuditFinding {
                code: "malformed_keyword_hijack".to_string(),
                severity: AuditSeverity::Critical,
                path: path.to_string(),
                detail: format!("static rule hijacked into keyword `{}`: {param}", kw.name),
            });
        }
    }
}

fn check_instruction_structure(
    ins: &InstructionAst,
    path: &str,
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    match ins {
        InstructionAst::Modal { modes, .. } => {
            if modes.is_empty() {
                critical.push(AuditFinding {
                    code: "empty_modal_modes".to_string(),
                    severity: AuditSeverity::Critical,
                    path: path.to_string(),
                    detail: "Modal instruction has zero modes".to_string(),
                });
            }
            for (mi, m) in modes.iter().enumerate() {
                for (bi, sub) in m.body.iter().enumerate() {
                    check_instruction_structure(
                        sub,
                        &format!("{path}.modes[{mi}].body[{bi}]"),
                        critical,
                        opaque,
                    );
                }
                if let Some(sub_face) = &m.granted {
                    audit_face_contents(
                        sub_face,
                        false,
                        &format!("{path}.modes[{mi}].granted"),
                        critical,
                        opaque,
                    );
                }
            }
        }
        InstructionAst::CreateEmblem(text) => {
            opaque.push(AuditFinding {
                code: "uncompiled_emblem".to_string(),
                severity: AuditSeverity::Opaque,
                path: path.to_string(),
                detail: text.clone(),
            });
        }
        InstructionAst::CreateEmblemFor { text, compiled, .. } => match compiled {
            Some(sub_face) => {
                audit_face_contents(
                    sub_face,
                    false,
                    &format!("{path}.compiled"),
                    critical,
                    opaque,
                );
            }
            None => {
                opaque.push(AuditFinding {
                    code: "uncompiled_emblem".to_string(),
                    severity: AuditSeverity::Opaque,
                    path: path.to_string(),
                    detail: text.clone(),
                });
            }
        },
        InstructionAst::GrantAbilityText {
            ability_text,
            compiled,
            ..
        } => match compiled {
            Some(sub_face) => {
                audit_face_contents(
                    sub_face,
                    false,
                    &format!("{path}.compiled"),
                    critical,
                    opaque,
                );
            }
            None if ability_text.len() > 2 => {
                opaque.push(AuditFinding {
                    code: "uncompiled_granted_ability".to_string(),
                    severity: AuditSeverity::Opaque,
                    path: path.to_string(),
                    detail: ability_text.clone(),
                });
            }
            None => {}
        },
        InstructionAst::TemporaryStatic { static_ability, .. } => {
            check_static_structure(
                static_ability,
                &format!("{path}.static_ability"),
                critical,
                opaque,
            );
        }
        InstructionAst::TemporaryReplacement { replacement, .. } => {
            check_replacement_structure(
                replacement,
                &format!("{path}.replacement"),
                critical,
                opaque,
            );
        }
        InstructionAst::CreateTokenWithAbilities {
            compiled_abilities, ..
        } => {
            audit_face_contents(
                compiled_abilities,
                false,
                &format!("{path}.compiled_abilities"),
                critical,
                opaque,
            );
        }
        InstructionAst::ExtremeLifePlayerAction { action, .. } => {
            check_instruction_structure(action, &format!("{path}.action"), critical, opaque);
        }
        InstructionAst::InteractiveChoice(InteractiveChoiceKind::VillainousChoice {
            options,
            ..
        }) => {
            for (oi, opt_body) in options.iter().enumerate() {
                for (i, sub) in opt_body.iter().enumerate() {
                    check_instruction_structure(
                        sub,
                        &format!("{path}.villainous[{oi}][{i}]"),
                        critical,
                        opaque,
                    );
                }
            }
        }
        InstructionAst::Optional(body)
        | InstructionAst::OptionalPlayer { body, .. }
        | InstructionAst::IfPreviousSucceeded(body)
        | InstructionAst::IfPreviousFailed(body)
        | InstructionAst::Otherwise(body)
        | InstructionAst::ReplaceEffect(body)
        | InstructionAst::ForEachPlayer { body, .. }
        | InstructionAst::ForEachTargetPlayer { body, .. }
        | InstructionAst::ForEachMatchingPlayer { body, .. }
        | InstructionAst::ForEachSubjectPlayer { body, .. }
        | InstructionAst::ForEachColorAmong { body, .. }
        | InstructionAst::ForEachObject { body, .. }
        | InstructionAst::ForEachCount { body, .. }
        | InstructionAst::DelayedTrigger { body, .. }
        | InstructionAst::DelayedEventTrigger { body, .. }
        | InstructionAst::TemporaryTrigger { body, .. }
        | InstructionAst::ReflexiveTrigger(body)
        | InstructionAst::BindX { body, .. }
        | InstructionAst::BindXY { body, .. }
        | InstructionAst::SpreeMode { body, .. }
        | InstructionAst::WhenYouNext { body, .. }
        | InstructionAst::TemporarySpecialAction { body, .. }
        | InstructionAst::ForAsLongAs { body, .. }
        | InstructionAst::ScopedDuration { body, .. }
        | InstructionAst::ScopedDurationText { body, .. } => {
            for (i, sub) in body.iter().enumerate() {
                check_instruction_structure(sub, &format!("{path}.body[{i}]"), critical, opaque);
            }
        }
        InstructionAst::IfCondition { then_body, .. } => {
            for (i, sub) in then_body.iter().enumerate() {
                check_instruction_structure(
                    sub,
                    &format!("{path}.then_body[{i}]"),
                    critical,
                    opaque,
                );
            }
        }
        InstructionAst::Unless { body, .. } => {
            for (i, sub) in body.iter().enumerate() {
                check_instruction_structure(sub, &format!("{path}.body[{i}]"), critical, opaque);
            }
        }
        InstructionAst::Choice(branches) => {
            for (bi, br) in branches.iter().enumerate() {
                for (i, sub) in br.iter().enumerate() {
                    check_instruction_structure(
                        sub,
                        &format!("{path}.choice[{bi}][{i}]"),
                        critical,
                        opaque,
                    );
                }
            }
        }
        _ => {}
    }
}

fn check_static_structure(
    st: &StaticAbilityAst,
    path: &str,
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    match st {
        StaticAbilityAst::DieRollBand { payload, .. }
        | StaticAbilityAst::StationBand { payload, .. }
        | StaticAbilityAst::StickerOption { payload, .. } => {
            opaque.push(AuditFinding {
                code: "uncompiled_band_payload".to_string(),
                severity: AuditSeverity::Opaque,
                path: path.to_string(),
                detail: payload.clone(),
            });
        }
        StaticAbilityAst::CompiledStationBand { compiled, .. }
        | StaticAbilityAst::CompiledStickerOption { compiled, .. } => {
            if let Some(sub_face) = compiled {
                audit_face_contents(
                    sub_face,
                    false,
                    &format!("{path}.compiled"),
                    critical,
                    opaque,
                );
            }
        }
        StaticAbilityAst::CompiledDieRollBand { body, .. } => {
            for (i, ins) in body.iter().enumerate() {
                check_instruction_structure(ins, &format!("{path}.body[{i}]"), critical, opaque);
            }
        }
        StaticAbilityAst::GrantAbilityText {
            ability_text,
            compiled,
            ..
        } => match compiled {
            Some(sub_face) => {
                audit_face_contents(
                    sub_face,
                    false,
                    &format!("{path}.compiled"),
                    critical,
                    opaque,
                );
            }
            None if ability_text.len() > 2 => {
                opaque.push(AuditFinding {
                    code: "uncompiled_granted_ability".to_string(),
                    severity: AuditSeverity::Opaque,
                    path: path.to_string(),
                    detail: ability_text.clone(),
                });
            }
            None => {}
        },
        StaticAbilityAst::BecomesEquipmentWith {
            granted_text,
            granted_abilities,
            ..
        }
        | StaticAbilityAst::LoseLandTypesAndGain {
            granted_text,
            granted_abilities,
            ..
        } => match granted_abilities {
            Some(sub_face) => {
                audit_face_contents(
                    sub_face,
                    false,
                    &format!("{path}.granted_abilities"),
                    critical,
                    opaque,
                );
            }
            None if !granted_text.trim().is_empty() => {
                opaque.push(AuditFinding {
                    code: "uncompiled_granted_ability".to_string(),
                    severity: AuditSeverity::Opaque,
                    path: path.to_string(),
                    detail: granted_text.clone(),
                });
            }
            None => {}
        },
        StaticAbilityAst::PairedTrigger { body, .. } | StaticAbilityAst::FollowUp(body) => {
            for (i, ins) in body.iter().enumerate() {
                check_instruction_structure(ins, &format!("{path}.body[{i}]"), critical, opaque);
            }
        }
        StaticAbilityAst::Compound(list) => {
            for (i, sub) in list.iter().enumerate() {
                check_static_structure(sub, &format!("{path}.compound[{i}]"), critical, opaque);
            }
        }
        StaticAbilityAst::OncePerTurn(inner) | StaticAbilityAst::OnceDuringYourTurn(inner) => {
            check_static_structure(inner, path, critical, opaque);
        }
        _ => {}
    }
}

fn looks_like_rust_debug_leak(s: &str, path: &str) -> bool {
    if s.contains("TargetObject {")
        || s.contains("Group(ObjectFilterAst")
        || s.contains("All(ObjectFilterAst")
        || s.contains("ObjectFilterAst {")
        || s.contains("SubjectRefAst")
        || s.contains("PlayerRefAst")
        || s.contains("ValueExprAst")
        || s.contains("TokenSpecAst")
        || s.contains("ForAsLongAs(")
        || s.contains("Anaphora(\"")
        || s.contains("Const(")
        || s.contains("fight:")
        || s.contains("regenerates:")
        || s.contains("earthbend:")
        || s.contains("dealt_damage_by:")
        || s.contains("top_Const")
        || s.contains("top_ChosenX")
        || s.starts_with("ref:Target")
        || s.starts_with("ref:Each")
        || s.starts_with("ref:Any")
        || s.starts_with("ref:That")
        || s.starts_with("ref:You")
    {
        return true;
    }
    if (path.ends_with(".GrantCastPermission.duration")
        || path.ends_with(".GrantModifiedCastPermission.duration"))
        && matches!(
            s,
            "UntilEndOfTurn"
                | "UntilYourNextTurn"
                | "UntilEndOfNextTurn"
                | "UntilEndOfCombat"
                | "UntilSourceLeaves"
                | "Permanent"
                | "Instant"
        )
    {
        return true;
    }
    false
}

fn walk_json_invariants(
    v: &Value,
    path: &str,
    face_idx: Option<usize>,
    face_oracles_low: &[String],
    critical: &mut Vec<AuditFinding>,
    opaque: &mut Vec<AuditFinding>,
) {
    match v {
        Value::Object(map) => {
            // Check ObjectFilterAst MODIFIED flag against face oracle text
            if let Some(flags_num) = map.get("flags").and_then(Value::as_u64)
                && map.contains_key("any_types")
                && map.contains_key("forbidden_types")
                && (flags_num & u64::from(ObjectFilterFlags::MODIFIED.0)) != 0
            {
                let face_mentions_modified = face_idx
                    .and_then(|i| face_oracles_low.get(i))
                    .is_some_and(|ot| ot.contains("modified"));
                if !face_mentions_modified {
                    critical.push(AuditFinding {
                        code: "spurious_modified_flag".to_string(),
                        severity: AuditSeverity::Critical,
                        path: path.to_string(),
                        detail: "ObjectFilterFlags::MODIFIED set on face without 'modified' text"
                            .to_string(),
                    });
                }
            }

            // Check raw quoted abilities stuffed into TokenSpecAst::Creature.keywords
            if map.contains_key("power")
                && map.contains_key("toughness")
                && let Some(kws) = map.get("keywords").and_then(Value::as_array)
            {
                for kw in kws {
                    if let Some(ks) = kw.as_str()
                        && ks.contains('"')
                    {
                        opaque.push(AuditFinding {
                            code: "uncompiled_token_ability".to_string(),
                            severity: AuditSeverity::Opaque,
                            path: format!("{path}.keywords"),
                            detail: ks.to_string(),
                        });
                    }
                }
            }

            if map.len() == 1
                && let Some((k, inner)) = map.iter().next()
            {
                match k.as_str() {
                    "StructuredRule" | "GameStateFlag" => {
                        critical.push(AuditFinding {
                            code: format!("legacy_banned_variant:{k}"),
                            severity: AuditSeverity::Critical,
                            path: path.to_string(),
                            detail: inner.to_string(),
                        });
                    }
                    "Choice" => {
                        if let Some(arr) = inner.as_array()
                            && arr.iter().all(Value::is_string)
                        {
                            for item in arr {
                                if let Some(ms) = item.as_str()
                                    && !is_mana_sequence(ms)
                                {
                                    critical.push(AuditFinding {
                                        code: "bad_mana_choice".to_string(),
                                        severity: AuditSeverity::Critical,
                                        path: format!("{path}.Choice"),
                                        detail: format!("non-mana-sequence choice entry: {ms:?}"),
                                    });
                                }
                            }
                        }
                    }
                    "EventOccurredThisTurn" => {
                        if let Some(ev) = inner.get("event").and_then(Value::as_str) {
                            opaque.push(AuditFinding {
                                code: "opaque:EventOccurredThisTurn".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: ev.to_string(),
                            });
                        }
                    }
                    "PlayerActionThisTurn" => {
                        if let Some(act) = inner.get("action").and_then(Value::as_str)
                            && act.contains(' ')
                        {
                            opaque.push(AuditFinding {
                                code: "opaque:PlayerActionThisTurn".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: act.to_string(),
                            });
                        }
                    }
                    "HasStatus" => {
                        if let Some(st) = inner.get("status").and_then(Value::as_str)
                            && st.contains(':')
                        {
                            opaque.push(AuditFinding {
                                code: "opaque:HasStatus_complex".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: st.to_string(),
                            });
                        }
                    }
                    "StateQualification" => {
                        if let Some(sq) = inner.as_str()
                            && sq.contains(' ')
                        {
                            opaque.push(AuditFinding {
                                code: "opaque:StateQualification_prose".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: sq.to_string(),
                            });
                        }
                    }
                    "AbilityRestriction" => {
                        if let Some(r) = inner.get("restriction").and_then(Value::as_str)
                            && r.contains(' ')
                        {
                            opaque.push(AuditFinding {
                                code: "opaque:AbilityRestriction_prose".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: r.to_string(),
                            });
                        }
                    }
                    "RuleOrCombatRestriction" => {
                        if let Some(r) = inner.get("rule").and_then(Value::as_str)
                            && r.contains(' ')
                        {
                            opaque.push(AuditFinding {
                                code: "opaque:RuleOrCombatRestriction_prose".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: r.to_string(),
                            });
                        }
                    }
                    "EventInstead" => {
                        if inner.get("instead_body").is_none_or(Value::is_null) {
                            let would = inner
                                .get("would")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            let instead = inner
                                .get("instead")
                                .and_then(Value::as_str)
                                .unwrap_or_default();
                            opaque.push(AuditFinding {
                                code: "opaque:EventInstead_unparsed".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: format!("would `{would}` -> instead `{instead}`"),
                            });
                        }
                    }
                    "PlayOrCastPermission" => {
                        if let Some(perm) = inner.get("permission").and_then(Value::as_str)
                            && (perm.contains(". ") || perm.len() > 48)
                        {
                            opaque.push(AuditFinding {
                                code: "opaque:PlayOrCastPermission_prose".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: perm.to_string(),
                            });
                        }
                    }
                    "ScopedDurationText" => {
                        if let Some(dt) = inner.get("duration_text").and_then(Value::as_str) {
                            opaque.push(AuditFinding {
                                code: "opaque:ScopedDurationText".to_string(),
                                severity: AuditSeverity::Opaque,
                                path: path.to_string(),
                                detail: dt.to_string(),
                            });
                        }
                    }
                    _ => {}
                }
            }

            for (k, val) in map {
                let next_path = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                if k == "faces"
                    && let Some(arr) = val.as_array()
                {
                    for (fi, fv) in arr.iter().enumerate() {
                        walk_json_invariants(
                            fv,
                            &format!("faces[{fi}]"),
                            Some(fi),
                            face_oracles_low,
                            critical,
                            opaque,
                        );
                    }
                    continue;
                }
                walk_json_invariants(
                    val,
                    &next_path,
                    face_idx,
                    face_oracles_low,
                    critical,
                    opaque,
                );
            }
        }
        Value::Array(arr) => {
            for item in arr {
                walk_json_invariants(item, path, face_idx, face_oracles_low, critical, opaque);
            }
        }
        Value::String(s) if looks_like_rust_debug_leak(s, path) => {
            critical.push(AuditFinding {
                code: "rust_debug_leak".to_string(),
                severity: AuditSeverity::Critical,
                path: path.to_string(),
                detail: s.clone(),
            });
        }
        _ => {}
    }
}
