//! Guarded source Program observations for offline reference readers.
//! This exporter uses the existing registry and serializer without starting a game.

use serde_json::{Value, json};
use spellwake_card_programs::OracleId;
use spellwake_engine::{
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
};
use spellwake_reference_tools::{
    creature_types, creature_vocabulary, ordinary_game::optional::OptionalSource, program,
};
use std::{collections::BTreeMap, path::PathBuf};

/// Export actual immutable Registry8 source19 Programs through the one canonical
/// serializer. The separate primary review must accept these complete Programs
/// and nested templates before a reader uses the export as value authority.
#[test]
#[ignore = "requires exact saved full source, genuine vocabulary and explicit private output path"]
fn optional_source_programs_export_from_guarded_registry8() -> Result<(), String> {
    let required_path = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("explicit {name} required"))
    };
    let metadata = required_path("SPELLWAKE_CATALOGUE_METADATA")?;
    let snapshot = required_path("SPELLWAKE_CATALOGUE_INPUT")?;
    let output = required_path("SPELLWAKE_OPTIONAL_PROGRAM_EXPORT_OUTPUT")?;
    if output.exists() {
        return Err("source Program export output already exists".into());
    }
    let vocabulary = creature_types::selected_vocabulary()?;
    let source = OptionalSource::load(&metadata, &snapshot, &vocabulary)?;
    // This is the exact same compile-time C03 input read and strictly admitted by
    // OptionalSource. It supplies source metadata, never expected game outputs.
    let input: Value =
        serde_json::from_str(include_str!("../src/ordinary_game/optional-inputs-v1.json"))
            .map_err(|error| format!("C03 export input: {error}"))?;
    let definitions = input["definitions"]
        .as_array()
        .ok_or("C03 source definitions must be an array")?;
    let registry = source.registry();
    let coverage = registry.coverage();
    let mut rows = BTreeMap::new();
    for definition in definitions {
        let oracle = definition["oracleId"]
            .as_str()
            .ok_or("C03 source definition lacks Oracle identity")?;
        let id = OracleId::try_from(oracle.to_owned()).map_err(|error| error.to_string())?;
        let native_row = coverage
            .cards
            .iter()
            .find(|row| row.oracle_id.as_deref() == Some(oracle))
            .ok_or("guarded Registry8 source row missing")?;
        // Registry::seed is the existing read path for its private bound Program.
        // This stand-in input handle is not emitted or counted as a physical card
        // or actual game; no GameState, policy, action or replay is constructed.
        let seed = registry
            .seed(&id, CardId(1), SeatId(41), ObjectDestination::LibraryBottom)
            .map_err(|error| format!("guarded source Program: {error}"))?;
        let row = json!({
            "source_definition": definition,
            "registry_binding": native_row,
            "program": program::view(&seed.program),
        });
        if rows.insert(oracle.to_owned(), row).is_some() {
            return Err("duplicate guarded source Program identity".into());
        }
    }
    if rows.len() != 19 {
        return Err("guarded source Program export requires exact source19".into());
    }
    let document = json!({
        "schema_version": 1,
        "kind": "guarded-source19-program-observation-v1",
        "profile": "optional-payment-v1",
        "registry_version": 8,
        "catalogue_sha256": source.inventory().compressed_sha256.as_str(),
        "vocabulary": creature_vocabulary::view(&vocabulary),
        "epochs": input["epochs"],
        "token_template_references": input["token_templates"],
        "rows": rows.into_values().collect::<Vec<_>>(),
        "limits": "Source Program observation only; separate whole-clause primary review, reader corruption checks and actual game/reference qualification remain required.",
    });
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("source Program export serialization: {error}"))?;
    // Keep prior exports immutable at the artifact boundary.
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|error| format!("source Program export create: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("source Program export write: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("source Program export newline: {error}"))?;
    Ok(())
}
