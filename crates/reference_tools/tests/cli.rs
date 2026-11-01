//! Process-level checks protect the report-to-exit-status boundary used by CI.
use std::process::Command;

#[test]
fn cli_rejects_mismatches_invalid_input_and_unreadable_fixtures() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("suite.json");
    let mut fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/scenarios/distributed-damage.json"
    ))
    .unwrap();
    fixture["cases"][0]["expected"] =
        serde_json::json!({"kind":"rejected","code":"incorrect-total"});
    std::fs::write(&path, fixture.to_string()).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_spellwake-reference-tools"))
            .arg(&path)
            .output()
            .unwrap()
    };
    let mismatch = run();
    assert!(!mismatch.status.success());
    let report: serde_json::Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    assert_eq!(report["data"]["failed"], 1);

    std::fs::write(&path, "invalid json").unwrap();
    let invalid = run();
    assert!(!invalid.status.success());
    let report: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(report["status"], "fail");
    assert!(report["data"]["fixture"].is_string());

    std::fs::remove_file(&path).unwrap();
    let unreadable = run();
    assert!(!unreadable.status.success());
    let report: serde_json::Value = serde_json::from_slice(&unreadable.stdout).unwrap();
    assert_eq!(report["status"], "error");
    assert_eq!(report["code"], "fixture-io-error");
}
