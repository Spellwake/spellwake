//! Single-card custom compiler and validator for Spellwake Open Play Custom Card Studio.
//!
//! Runs both Stage 1 (`spellwake-oracle-compiler` strict AST parse + readable AST chips) and
//! Stage 2 (`spellwake-card-programs` `Profile::Current` lowering + `Program::validate`),
//! returning an honest playability verdict, deterministic rules-only SHA-256 fingerprint,
//! and optionally the boxed executable `Program` ready for match-scoped `NativeProgramOverlay`.

use crate::catalog_support::SUPPORT_COMPILER_REVISION;
use crate::keyword_creatures::compile_current_card_program;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spellwake_content::{CatalogFace, Inventory, InventoryCard};
use spellwake_contracts::{creature_types::CreatureTypeVocabulary, names::EnglishCardName};
use spellwake_engine::{
    characteristics::Color,
    effects::Program,
    program::{ArtifactSubtype, EnchantmentSubtype, PlaneswalkerSubtype, SpellSubtype},
};
use spellwake_oracle_compiler::{
    CardInput, FaceInput, Vocabulary as OracleVocabulary,
    v2::{
        compile_card_v2,
        ir::{Ability as V2Ability, CompiledCardV2, CompiledFaceV2, LoyaltyDelta},
    },
};
use std::collections::{BTreeMap, BTreeSet};

/// Rules-relevant printed fields of a single custom card face.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomCardFaceRules {
    pub name: String,
    pub mana_cost: Option<String>,
    pub type_line: String,
    pub oracle_text: String,
    pub power: Option<String>,
    pub toughness: Option<String>,
    pub loyalty: Option<String>,
    pub defense: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_indicator: Option<Vec<String>>,
}

/// Slim rules-only compilation input for one custom card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomCardCompileInput {
    pub layout: String,
    pub rarity: String,
    pub faces: Vec<CustomCardFaceRules>,
}

/// Severity of a compilation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// Compiler stage that emitted a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStage {
    Metadata,
    Stage1,
    Stage2,
    Validation,
}

/// Structured diagnostic item surfaced beneath the Studio rules textarea.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileDiagnostic {
    pub severity: DiagnosticSeverity,
    pub stage: DiagnosticStage,
    pub face_index: Option<usize>,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clause_text: Option<String>,
}

/// Category of a concise human-readable AST reading chip in the Studio dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AstReadingKind {
    Keyword,
    Triggered,
    Activated,
    Static,
    Replacement,
    SpellEffect,
    Loyalty,
    SagaChapter,
    ClassLevel,
}

/// Concise verified AST reading item derived directly from Stage 1 `CardAst`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AstReadingItem {
    pub face_index: usize,
    pub kind: AstReadingKind,
    pub label: String,
    pub summary: String,
}

/// Wire-serializable compilation report returned to Studio and saved to Postgres.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomCardCompileReport {
    pub rules_sha256: String,
    pub compiler_revision: String,
    pub stage1_parsed: bool,
    pub stage2_playable: bool,
    pub diagnostics: Vec<CompileDiagnostic>,
    pub ast_reading: Vec<AstReadingItem>,
    pub color_identity: Vec<String>,
    pub mana_value: u32,
}

/// Full result of [`compile_custom_card`], pairing the public report with the boxed `Program`.
#[derive(Debug, Clone)]
pub struct CustomCardCompiled {
    pub report: CustomCardCompileReport,
    pub program: Option<Box<Program>>,
}

/// Compute the deterministic rules-only SHA-256 fingerprint for a custom card.
/// Excludes visual frame accents, flavor text, collector notes, and art bytes.
#[must_use]
pub fn compute_rules_sha256(input: &CustomCardCompileInput) -> String {
    #[derive(Serialize)]
    struct Canonical<'a> {
        version: u32,
        layout: &'a str,
        rarity: &'a str,
        faces: &'a [CustomCardFaceRules],
    }
    let bytes = serde_json::to_vec(&Canonical {
        version: 1,
        layout: &input.layout,
        rarity: &input.rarity,
        faces: &input.faces,
    })
    .unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

/// Normalize Studio visual layouts (`planeswalker`, `saga`) to engine layout `"normal"`.
#[must_use]
pub fn normalize_engine_layout(layout: &str) -> &str {
    match layout {
        "planeswalker" | "saga" => "normal",
        other => other,
    }
}

/// Compile and validate one custom card through Stage 1 (`oracle_compiler`) and Stage 2
/// (`card_programs` `Profile::Current` + `Program::validate`).
///
/// Callers in async web runtimes MUST execute this on a thread with at least a 16 MiB stack.
#[inline(never)]
#[must_use]
pub fn compile_custom_card(
    input: &CustomCardCompileInput,
    vocabulary: &CreatureTypeVocabulary,
) -> CustomCardCompiled {
    let rules_sha256 = compute_rules_sha256(input);
    let mut diagnostics = Vec::new();

    let expected_faces = match input.layout.as_str() {
        "normal" | "planeswalker" | "saga" => 1,
        "transform" | "modal_dfc" | "adventure" | "split" => 2,
        other => {
            diagnostics.push(CompileDiagnostic {
                severity: DiagnosticSeverity::Error,
                stage: DiagnosticStage::Metadata,
                face_index: None,
                code: "unsupported-layout".to_owned(),
                message: format!("Unsupported layout '{other}'."),
                clause_text: None,
            });
            return CustomCardCompiled {
                report: CustomCardCompileReport {
                    rules_sha256,
                    compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
                    stage1_parsed: false,
                    stage2_playable: false,
                    diagnostics,
                    ast_reading: Vec::new(),
                    color_identity: Vec::new(),
                    mana_value: 0,
                },
                program: None,
            };
        }
    };

    if input.faces.len() != expected_faces {
        diagnostics.push(CompileDiagnostic {
            severity: DiagnosticSeverity::Error,
            stage: DiagnosticStage::Metadata,
            face_index: None,
            code: "layout-face-count-mismatch".to_owned(),
            message: format!(
                "Layout '{}' requires {} face(s), got {}.",
                input.layout,
                expected_faces,
                input.faces.len()
            ),
            clause_text: None,
        });
        return CustomCardCompiled {
            report: CustomCardCompileReport {
                rules_sha256,
                compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
                stage1_parsed: false,
                stage2_playable: false,
                diagnostics,
                ast_reading: Vec::new(),
                color_identity: Vec::new(),
                mana_value: 0,
            },
            program: None,
        };
    }

    let mut metadata_valid = true;
    for (idx, face) in input.faces.iter().enumerate() {
        if EnglishCardName::new(face.name.clone()).is_err() {
            metadata_valid = false;
            diagnostics.push(CompileDiagnostic {
                severity: DiagnosticSeverity::Error,
                stage: DiagnosticStage::Metadata,
                face_index: Some(idx),
                code: "invalid-card-name".to_owned(),
                message: "Card name must be a non-empty trimmed English card name without control characters.".to_owned(),
                clause_text: Some(face.name.clone()),
            });
        }
        validate_face_type_line(idx, face, vocabulary, &mut diagnostics, &mut metadata_valid);
    }

    let full_name = if input.faces.len() == 2 {
        format!("{} // {}", input.faces[0].name, input.faces[1].name)
    } else {
        input.faces[0].name.clone()
    };
    let engine_layout = normalize_engine_layout(&input.layout);
    let color_identity = derive_color_identity(&input.faces);
    let mana_value = parse_mana_value(input.faces[0].mana_cost.as_deref());

    // Stage 1: strict oracle_compiler AST parse.
    let oracle_vocab = OracleVocabulary::from_creature_members(vocabulary.members());
    let face_inputs: Vec<FaceInput<'_>> = input
        .faces
        .iter()
        .map(|f| FaceInput {
            name: &f.name,
            type_line: &f.type_line,
            oracle_text: &f.oracle_text,
        })
        .collect();
    let card_input = CardInput {
        oracle_id: "00000000-0000-4000-8000-000000000000",
        name: &full_name,
        layout: engine_layout,
        faces: face_inputs,
    };
    let ast = compile_card_v2(&card_input, &oracle_vocab);
    let ast_reading = extract_ast_reading(&ast);
    let stage1_parsed = metadata_valid && ast.strictly_compiled;
    for (face_idx, face_ast) in ast.faces.iter().enumerate() {
        for err in &face_ast.errors {
            let msg: String = format!("Could not parse clause ({}): {}", err.message, err.text)
                .chars()
                .take(512)
                .collect();
            let clause: String = err.text.chars().take(1024).collect();
            diagnostics.push(CompileDiagnostic {
                severity: DiagnosticSeverity::Warning,
                stage: DiagnosticStage::Stage1,
                face_index: Some(face_idx),
                code: format!("ast-{:?}", err.stage).to_ascii_lowercase(),
                message: msg,
                clause_text: Some(clause),
            });
        }
    }

    if !metadata_valid {
        return CustomCardCompiled {
            report: CustomCardCompileReport {
                rules_sha256,
                compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
                stage1_parsed: false,
                stage2_playable: false,
                diagnostics,
                ast_reading,
                color_identity,
                mana_value,
            },
            program: None,
        };
    }

    // Build synthetic single-card Inventory with loyalties, defenses (including single-faced
    // Battles), and back color indicator so Profile::Current lowering has full face metadata.
    let synthetic_oracle = "00000000-0000-4000-8000-000000000001".to_owned();
    let catalog_faces: Vec<CatalogFace> = input
        .faces
        .iter()
        .map(|f| CatalogFace {
            name: f.name.clone(),
            type_line: Some(f.type_line.clone()),
            oracle_text: Some(f.oracle_text.clone()),
            mana_cost: Some(f.mana_cost.clone().unwrap_or_default()),
            power: f.power.clone(),
            toughness: f.toughness.clone(),
        })
        .collect();
    let card = InventoryCard {
        scryfall_id: synthetic_oracle.clone(),
        oracle_id: Some(synthetic_oracle),
        name: full_name,
        layout: engine_layout.to_owned(),
        faces: catalog_faces,
        legalities: BTreeMap::new(),
        eligible_formats: Vec::new(),
        required_by_base_formats: false,
        program_evidence: "custom-card-open-play",
    };
    let loyalties: Vec<Option<String>> = input.faces.iter().map(|f| f.loyalty.clone()).collect();
    let defenses: Vec<Option<String>> = input.faces.iter().map(|f| f.defense.clone()).collect();
    let back_indicator = input
        .faces
        .get(1)
        .and_then(|f| f.color_indicator.clone())
        .filter(|v| !v.is_empty());
    let inventory = Inventory::single_custom(
        card,
        loyalties,
        defenses,
        back_indicator,
        color_identity.clone(),
    );

    let mut nonland_names = None;
    let lowered = compile_current_card_program(
        &inventory.cards[0],
        &inventory,
        vocabulary,
        &mut nonland_names,
    );

    let (stage2_playable, program) = match lowered {
        Ok(mut prog) => {
            // Apply explicit front-face color indicator if supplied by the author.
            if let Some(front_ind) = input.faces[0].color_indicator.as_ref()
                && !front_ind.is_empty()
            {
                let set = prog.color_indicator.get_or_insert_with(BTreeSet::new);
                for symbol in front_ind {
                    if let Some(color) = parse_color_symbol(symbol) {
                        set.insert(color);
                    }
                }
                if let Err(err) = prog.validate(vocabulary) {
                    diagnostics.push(CompileDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        stage: DiagnosticStage::Validation,
                        face_index: Some(0),
                        code: "color-indicator-validation".to_owned(),
                        message: format!("Color indicator validation failed: {err:?}"),
                        clause_text: None,
                    });
                    return CustomCardCompiled {
                        report: CustomCardCompileReport {
                            rules_sha256,
                            compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
                            stage1_parsed,
                            stage2_playable: false,
                            diagnostics,
                            ast_reading,
                            color_identity,
                            mana_value,
                        },
                        program: None,
                    };
                }
            }
            (true, Some(prog))
        }
        Err(reason) => {
            diagnostics.push(CompileDiagnostic {
                severity: DiagnosticSeverity::Warning,
                stage: DiagnosticStage::Stage2,
                face_index: None,
                code: reason.clone(),
                message: humanize_stage2_blocker(&reason),
                clause_text: None,
            });
            (false, None)
        }
    };

    CustomCardCompiled {
        report: CustomCardCompileReport {
            rules_sha256,
            compiler_revision: SUPPORT_COMPILER_REVISION.to_owned(),
            stage1_parsed,
            stage2_playable,
            diagnostics,
            ast_reading,
            color_identity,
            mana_value,
        },
        program,
    }
}

fn validate_face_type_line(
    face_index: usize,
    face: &CustomCardFaceRules,
    vocabulary: &CreatureTypeVocabulary,
    diagnostics: &mut Vec<CompileDiagnostic>,
    metadata_valid: &mut bool,
) {
    let type_line = face.type_line.trim();
    if type_line.is_empty() {
        *metadata_valid = false;
        diagnostics.push(CompileDiagnostic {
            severity: DiagnosticSeverity::Error,
            stage: DiagnosticStage::Metadata,
            face_index: Some(face_index),
            code: "empty-type-line".to_owned(),
            message: "Type line cannot be empty.".to_owned(),
            clause_text: None,
        });
        return;
    }

    let (left, right_opt) = type_line
        .split_once('\u{2014}')
        .or_else(|| type_line.split_once(" - "))
        .map_or((type_line, None), |(l, r)| (l.trim(), Some(r.trim())));

    let mut has_creature = false;
    let mut has_artifact = false;
    let mut has_enchantment = false;
    let mut has_planeswalker = false;
    let mut has_instant_or_sorcery = false;
    let mut has_land = false;
    let mut has_battle = false;
    let mut has_kindred = false;

    for word in left.split_whitespace() {
        match word {
            "Basic" | "Legendary" | "Snow" | "World" | "Ongoing" => {}
            "Creature" => has_creature = true,
            "Artifact" => has_artifact = true,
            "Enchantment" => has_enchantment = true,
            "Planeswalker" => has_planeswalker = true,
            "Instant" | "Sorcery" => has_instant_or_sorcery = true,
            "Land" => has_land = true,
            "Battle" => has_battle = true,
            "Kindred" | "Tribal" => has_kindred = true,
            other => {
                *metadata_valid = false;
                diagnostics.push(CompileDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    stage: DiagnosticStage::Metadata,
                    face_index: Some(face_index),
                    code: "unknown-card-type".to_owned(),
                    message: format!("Unknown card type or supertype '{other}'."),
                    clause_text: Some(type_line.to_owned()),
                });
            }
        }
    }

    if !has_creature
        && !has_artifact
        && !has_enchantment
        && !has_planeswalker
        && !has_instant_or_sorcery
        && !has_land
        && !has_battle
    {
        *metadata_valid = false;
        diagnostics.push(CompileDiagnostic {
            severity: DiagnosticSeverity::Error,
            stage: DiagnosticStage::Metadata,
            face_index: Some(face_index),
            code: "missing-card-type".to_owned(),
            message: "Type line must include at least one valid Magic card type.".to_owned(),
            clause_text: Some(type_line.to_owned()),
        });
    }

    if let Some(subtypes_str) = right_opt {
        for sub in subtypes_str.split_whitespace() {
            let valid_creature =
                (has_creature || has_kindred) && vocabulary.resolve_label(sub).is_some();
            let valid_artifact = has_artifact && ArtifactSubtype::from_wire_label(sub).is_some();
            let valid_enchantment = has_enchantment
                && (sub == "Aura" || EnchantmentSubtype::from_wire_label(sub).is_some());
            let valid_planeswalker =
                has_planeswalker && PlaneswalkerSubtype::from_label(sub).is_some();
            let valid_spell =
                has_instant_or_sorcery && SpellSubtype::from_wire_label(sub).is_some();
            let valid_land = has_land
                && matches!(
                    sub,
                    "Plains"
                        | "Island"
                        | "Swamp"
                        | "Mountain"
                        | "Forest"
                        | "Cave"
                        | "Desert"
                        | "Gate"
                        | "Lair"
                        | "Locus"
                        | "Mine"
                        | "Planet"
                        | "Power-Plant"
                        | "Sphere"
                        | "Tower"
                        | "Town"
                        | "Urza's"
                );
            let valid_battle = has_battle && sub == "Siege";

            if !(valid_creature
                || valid_artifact
                || valid_enchantment
                || valid_planeswalker
                || valid_spell
                || valid_land
                || valid_battle)
            {
                diagnostics.push(CompileDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    stage: DiagnosticStage::Metadata,
                    face_index: Some(face_index),
                    code: "unsupported-subtype".to_owned(),
                    message: format!(
                        "Subtype '{sub}' is not in Spellwake's supported subtype vocabulary for this card type."
                    ),
                    clause_text: Some(type_line.to_owned()),
                });
            }
        }
    }
}

fn extract_ast_reading(ast: &CompiledCardV2) -> Vec<AstReadingItem> {
    let mut items = Vec::new();
    for (face_index, face) in ast.faces.iter().enumerate() {
        extract_face_ast_reading(face_index, face, &mut items);
    }
    items
}

fn extract_face_ast_reading(
    face_index: usize,
    face: &CompiledFaceV2,
    out: &mut Vec<AstReadingItem>,
) {
    for kw in &face.keywords {
        let name = format!("{:?}", kw.kind);
        let summary = match &kw.param {
            Some(param) => format!("{name} {}", truncate_debug(param, 64)),
            None => name.clone(),
        };
        out.push(AstReadingItem {
            face_index,
            kind: AstReadingKind::Keyword,
            label: name,
            summary,
        });
    }
    for ab in &face.abilities {
        match ab {
            V2Ability::Triggered { trigger, body } => {
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::Triggered,
                    label: "Triggered".to_owned(),
                    summary: format!("{:?} -> {} step(s)", trigger.events, body.len()),
                });
            }
            V2Ability::Activated { cost, body, .. } => {
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::Activated,
                    label: "Activated".to_owned(),
                    summary: format!("{} cost atom(s) -> {} step(s)", cost.len(), body.len()),
                });
            }
            V2Ability::Loyalty { delta, body } => {
                let d = match delta {
                    LoyaltyDelta::Fixed(n) if *n > 0 => format!("+{n}"),
                    LoyaltyDelta::Fixed(n) => format!("{n}"),
                    LoyaltyDelta::MinusX => "-X".to_owned(),
                };
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::Loyalty,
                    label: format!("Loyalty {d}"),
                    summary: format!("{d}: {} step(s)", body.len()),
                });
            }
            V2Ability::SagaChapter { chapters, body } => {
                let chs = chapters
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::SagaChapter,
                    label: format!("Chapter {chs}"),
                    summary: format!("{} step(s)", body.len()),
                });
            }
            V2Ability::TableRow { min, max, .. } => {
                let label = match max {
                    Some(hi) => format!("Row {min}-{hi}"),
                    None => format!("Threshold {min}+"),
                };
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::ClassLevel,
                    label: label.clone(),
                    summary: label,
                });
            }
            V2Ability::StickerRow { ticket_cost, .. } => {
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::Static,
                    label: format!("Sticker ({ticket_cost} TK)"),
                    summary: format!("Sticker row ({ticket_cost} TK)"),
                });
            }
            V2Ability::CaseSolveCondition(_) => {
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::Static,
                    label: "To Solve".to_owned(),
                    summary: "Case solve condition".to_owned(),
                });
            }
            V2Ability::CaseSolved(_) => {
                out.push(AstReadingItem {
                    face_index,
                    kind: AstReadingKind::Static,
                    label: "Solved".to_owned(),
                    summary: "Case solved abilities".to_owned(),
                });
            }
        }
    }
    if !face.spell_effects.is_empty() {
        out.push(AstReadingItem {
            face_index,
            kind: AstReadingKind::SpellEffect,
            label: "Spell Effect".to_owned(),
            summary: format!("{} instruction(s)", face.spell_effects.len()),
        });
    }
    for st in &face.statics {
        out.push(AstReadingItem {
            face_index,
            kind: AstReadingKind::Static,
            label: "Static".to_owned(),
            summary: truncate_debug(st, 96),
        });
    }
    for rp in &face.replacements {
        out.push(AstReadingItem {
            face_index,
            kind: AstReadingKind::Replacement,
            label: "Replacement".to_owned(),
            summary: truncate_debug(rp, 96),
        });
    }
}

fn truncate_debug<T: std::fmt::Debug>(value: &T, max_len: usize) -> String {
    let raw = format!("{value:?}");
    let head = raw.split(['{', '(']).next().unwrap_or(&raw).trim();
    if head.len() <= max_len {
        head.to_owned()
    } else {
        head.chars().take(max_len).collect()
    }
}

fn parse_color_symbol(symbol: &str) -> Option<Color> {
    match symbol {
        "W" => Some(Color::White),
        "U" => Some(Color::Blue),
        "B" => Some(Color::Black),
        "R" => Some(Color::Red),
        "G" => Some(Color::Green),
        _ => None,
    }
}

fn derive_color_identity(faces: &[CustomCardFaceRules]) -> Vec<String> {
    let mut present: BTreeSet<&'static str> = BTreeSet::new();
    for face in faces {
        if let Some(cost) = face.mana_cost.as_deref() {
            collect_mana_colors(cost, &mut present);
        }
        collect_mana_colors(&face.oracle_text, &mut present);
        if let Some(indicator) = face.color_indicator.as_ref() {
            for c in indicator {
                match c.as_str() {
                    "W" => {
                        present.insert("W");
                    }
                    "U" => {
                        present.insert("U");
                    }
                    "B" => {
                        present.insert("B");
                    }
                    "R" => {
                        present.insert("R");
                    }
                    "G" => {
                        present.insert("G");
                    }
                    _ => {}
                }
            }
        }
    }
    ["W", "U", "B", "R", "G"]
        .into_iter()
        .filter(|c| present.contains(c))
        .map(str::to_owned)
        .collect()
}

fn collect_mana_colors(text: &str, out: &mut BTreeSet<&'static str>) {
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut token = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                token.push(inner);
            }
            for part in token.split('/') {
                match part.trim() {
                    "W" => {
                        out.insert("W");
                    }
                    "U" => {
                        out.insert("U");
                    }
                    "B" => {
                        out.insert("B");
                    }
                    "R" => {
                        out.insert("R");
                    }
                    "G" => {
                        out.insert("G");
                    }
                    _ => {}
                }
            }
        }
    }
}

fn parse_mana_value(mana_cost: Option<&str>) -> u32 {
    let Some(cost) = mana_cost else {
        return 0;
    };
    let mut total = 0_u32;
    let mut chars = cost.chars();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut token = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                token.push(inner);
            }
            let t = token.trim();
            if t.is_empty() || t == "X" || t == "Y" || t == "Z" {
                continue;
            }
            if let Ok(n) = t.parse::<u32>() {
                total = total.saturating_add(n);
            } else if t.starts_with("2/") {
                total = total.saturating_add(2);
            } else {
                total = total.saturating_add(1);
            }
        }
    }
    total
}

fn humanize_stage2_blocker(code: &str) -> String {
    match code {
        "invalid-english-name" => "Card name contains unsupported characters.".to_owned(),
        other if other.starts_with("validation:") => {
            format!("Engine validation rejected the compiled program ({other}).")
        }
        other => format!(
            "Parsed rules text could not yet be lowered to a playable Spellwake engine program ({other})."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vocab() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new(
            "2026-test".to_owned(),
            [0_u8; 32],
            vec![
                "Artificer".to_owned(),
                "Cat".to_owned(),
                "Dragon".to_owned(),
                "Elf".to_owned(),
                "Goblin".to_owned(),
                "Human".to_owned(),
                "Soldier".to_owned(),
                "Wizard".to_owned(),
            ],
        )
        .expect("valid test creature vocabulary")
    }

    #[test]
    fn compiles_vanilla_and_keyword_creature_on_16mib_stack() {
        let handle = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let vocab = test_vocab();
                let input = CustomCardCompileInput {
                    layout: "normal".to_owned(),
                    rarity: "rare".to_owned(),
                    faces: vec![CustomCardFaceRules {
                        name: "Aetherblade Vanguard".to_owned(),
                        mana_cost: Some("{2}{U}{R}".to_owned()),
                        type_line: "Creature \u{2014} Human Artificer".to_owned(),
                        oracle_text: "Flying, haste".to_owned(),
                        power: Some("3".to_owned()),
                        toughness: Some("2".to_owned()),
                        loyalty: None,
                        defense: None,
                        color_indicator: None,
                    }],
                };
                let compiled = compile_custom_card(&input, &vocab);
                assert!(compiled.report.stage1_parsed);
                assert!(compiled.report.stage2_playable);
                assert!(compiled.program.is_some());
                assert_eq!(compiled.report.mana_value, 4);
                assert_eq!(compiled.report.color_identity, vec!["U", "R"]);
            })
            .expect("spawn compiler thread");
        handle.join().expect("compiler thread succeeded");
    }

    #[test]
    fn compiles_instant_damage_spell_and_hashes_rules_only() {
        let handle = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let vocab = test_vocab();
                let input = CustomCardCompileInput {
                    layout: "normal".to_owned(),
                    rarity: "common".to_owned(),
                    faces: vec![CustomCardFaceRules {
                        name: "Storm Bolt".to_owned(),
                        mana_cost: Some("{R}".to_owned()),
                        type_line: "Instant".to_owned(),
                        oracle_text: "Storm Bolt deals 3 damage to any target.".to_owned(),
                        power: None,
                        toughness: None,
                        loyalty: None,
                        defense: None,
                        color_indicator: None,
                    }],
                };
                let compiled = compile_custom_card(&input, &vocab);
                assert!(compiled.report.stage1_parsed);
                assert!(compiled.report.stage2_playable);
                assert_eq!(compiled.report.rules_sha256.len(), 64);
                assert_eq!(compiled.report.mana_value, 1);
            })
            .expect("spawn compiler thread");
        handle.join().expect("compiler thread succeeded");
    }
}
