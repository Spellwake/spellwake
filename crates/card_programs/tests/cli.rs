//! Verify real saved-source/registry process envelopes and nonzero failure exits.
use std::{path::Path, process::Command};
#[test]
fn cli_reports_partial_coverage_and_rejects_invalid_or_unavailable_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = env!("CARGO_BIN_EXE_spellwake-card-programs");
    let vocabulary = spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
        "synthetic-registry-cli-unit".into(),
        [0; 32],
        vec!["Cat".into()],
    )
    .unwrap();
    let vocabulary_path = std::env::temp_dir().join(format!(
        "spellwake-registry-cli-V-{}.json",
        std::process::id()
    ));
    std::fs::write(&vocabulary_path,serde_json::to_vec(&serde_json::json!({"rules_revision":vocabulary.rules_revision(),"rules_sha256":"0".repeat(64),"members":vocabulary.members(),"id":vocabulary.id().to_string()})).unwrap()).unwrap();
    let output = Command::new(binary)
        .args([
            root.join("tests/catalogue/registry-cards.meta.json"),
            root.join("tests/catalogue/registry-cards.jsonl.gz"),
        ])
        .arg("--creature-vocabulary-input")
        .arg(&vocabulary_path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "success");
    assert_eq!(report["data"]["requiredPartialBindings"], 4);
    assert_eq!(report["data"]["fullyQualifiedCards"], 0);
    for (args, status) in [
        (vec![], "fail"),
        (vec!["missing-metadata", "missing-snapshot"], "error"),
    ] {
        let mut command = Command::new(binary);
        command.args(&args);
        if !args.is_empty() {
            command
                .arg("--creature-vocabulary-input")
                .arg(&vocabulary_path);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], status);
    }
    std::fs::remove_file(vocabulary_path).unwrap();
}
