//! Verify the real interaction binary consumes guarded catalogue programs.
use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};
use std::{
    io::{Read, Write},
    path::Path,
    process::Command,
};

#[test]
#[ignore = "full current capture requires explicit source-qualified V and actual D06 publication paths"]
fn default_subset_and_explicit_saved_snapshot_have_the_same_behavior_but_distinct_provenance() {
    let binary = env!("CARGO_BIN_EXE_interaction-trace");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let default = real_capture(binary).output().unwrap();
    assert!(default.status.success());
    let current: serde_json::Value = serde_json::from_slice(&default.stdout).unwrap();
    assert_eq!(current["schema_version"], 4);
    let subset = &current["observations"];
    let saved = real_capture(binary)
        .args([
            root.join("tests/catalogue/registry-cards.meta.json"),
            root.join("tests/catalogue/registry-cards.jsonl.gz"),
        ])
        .output()
        .unwrap();
    assert!(saved.status.success());
    let current: serde_json::Value = serde_json::from_slice(&saved.stdout).unwrap();
    let explicit = &current["observations"];
    assert_eq!(
        subset["data"]["registry_input"],
        "embedded-four-card-test-subset"
    );
    assert_eq!(explicit["data"]["registry_input"], "saved-snapshot");
    assert_eq!(
        subset["data"]["registry_source_sha256"],
        explicit["data"]["registry_source_sha256"]
    );
    assert_eq!(subset["data"]["scenarios"], explicit["data"]["scenarios"]);
}

#[test]
fn oracle_text_drift_stops_execution_and_bad_sources_never_fall_back_to_embedded_programs() {
    let mut raw = String::new();
    MultiGzDecoder::new(
        include_bytes!("../../../tests/catalogue/registry-cards.jsonl.gz").as_slice(),
    )
    .read_to_string(&mut raw)
    .unwrap();
    let raw = raw.replace("deals 3 damage", "deals 4 damage");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(raw.as_bytes()).unwrap();
    let changed = encoder.finish().unwrap();
    let mut metadata: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/catalogue/registry-cards.meta.json"
    ))
    .unwrap();
    metadata["compressed_size"] = changed.len().into();
    let temp = tempfile::tempdir().unwrap();
    let metadata_path = temp.path().join("metadata.json");
    let snapshot_path = temp.path().join("cards.jsonl.gz");
    std::fs::write(&metadata_path, metadata.to_string()).unwrap();
    std::fs::write(&snapshot_path, changed).unwrap();
    let binary = env!("CARGO_BIN_EXE_interaction-trace");
    let vocabulary_path = synthetic_input(temp.path());
    let mut command = Command::new(binary);
    command
        .env_remove("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .env_remove("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION")
        .arg("--creature-vocabulary-input")
        .arg(&vocabulary_path);
    let output = command
        .args([metadata_path, snapshot_path])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "fail");
    assert_eq!(response["data"]["registry"], "invalid-input");
    let output = Command::new(binary)
        .env_remove("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .env_remove("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION")
        .arg("--creature-vocabulary-input")
        .arg(&vocabulary_path)
        .args(["missing-metadata", "missing-snapshot"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "error");
}

#[test]
#[ignore = "full current capture requires explicit source-qualified V and actual D06 publication paths"]
fn copied_spell_profile_uses_guarded_five_card_source_and_never_falls_back() {
    let binary = env!("CARGO_BIN_EXE_interaction-trace");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let default = real_capture(binary).arg("--copies").output().unwrap();
    assert!(default.status.success());
    let current: serde_json::Value = serde_json::from_slice(&default.stdout).unwrap();
    let report = &current["observations"];
    assert_eq!(report["data"]["schema_version"], 3);
    assert_eq!(
        report["data"]["registry_input"],
        "embedded-five-card-test-subset"
    );
    let scenarios = report["data"]["scenarios"].as_array().unwrap();
    assert_eq!(scenarios.len(), 6);
    assert_eq!(
        scenarios
            .iter()
            .map(|case| case["observations"].as_array().unwrap().len())
            .sum::<usize>(),
        38
    );
    let saved = real_capture(binary)
        .arg("--copies")
        .args([
            root.join("tests/catalogue/spell-copy-cards.meta.json"),
            root.join("tests/catalogue/spell-copy-cards.jsonl.gz"),
        ])
        .output()
        .unwrap();
    assert!(saved.status.success());
    let current: serde_json::Value = serde_json::from_slice(&saved.stdout).unwrap();
    let explicit = &current["observations"];
    assert_eq!(report["data"]["scenarios"], explicit["data"]["scenarios"]);
    let missing = real_capture(binary)
        .arg("--copies")
        .args([
            root.join("tests/catalogue/registry-cards.meta.json"),
            root.join("tests/catalogue/registry-cards.jsonl.gz"),
        ])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    let failed: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(failed["status"], "fail");
    assert_eq!(failed["data"]["registry"], "invalid-input");
}

fn real_capture(binary: &str) -> Command {
    let input = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("explicit source-qualified vocabulary input required");
    std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION")
        .expect("actual native D06 publication required");
    let mut command = Command::new(binary);
    command.arg("--creature-vocabulary-input").arg(input);
    command
}
fn synthetic_input(directory: &Path) -> std::path::PathBuf {
    let vocabulary = spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
        "synthetic-registry-source-guard-unit".into(),
        [0; 32],
        vec!["Cat".into(), "Elf".into()],
    )
    .unwrap();
    let path = directory.join("synthetic-vocabulary.input.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&spellwake_reference_tools::projection::vocabulary(
            &vocabulary,
        ))
        .unwrap(),
    )
    .unwrap();
    path
}
