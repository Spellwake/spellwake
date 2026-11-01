//! Verify real saved-source/registry process envelopes and nonzero failure exits.
use std::{path::Path, process::Command};
#[test]
fn cli_reports_partial_coverage_and_rejects_invalid_or_unavailable_inputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = env!("CARGO_BIN_EXE_spellwake-card-programs");
    let output = Command::new(binary)
        .args([
            root.join("tests/catalogue/registry-cards.meta.json"),
            root.join("tests/catalogue/registry-cards.jsonl.gz"),
        ])
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
        let output = Command::new(binary).args(args).output().unwrap();
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["status"], status);
    }
}
