//! Actual CLI error envelopes; these do not reissue source-qualified game captures.
use serde_json::Value;
use std::{path::Path, process::Command};

fn invoke(binary: &str, args: &[&str]) -> Value {
    let vocabulary = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("explicit qualified vocabulary input required");
    let output = Command::new(binary)
        .args(args)
        .arg("--creature-vocabulary-input")
        .arg(vocabulary)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 10);
    assert_eq!(
        report["record_codec"],
        "spellwake.current-native-qualification.v2"
    );
    assert_eq!(report["status"], "failed");
    report
}

#[test]
fn current_cli_usage_errors_use_the_common_report_epoch() {
    for binary in [
        env!("CARGO_BIN_EXE_ordinary-trigger"),
        env!("CARGO_BIN_EXE_resolution-values"),
    ] {
        let report = invoke(binary, &[]);
        assert!(report["failure"].as_str().unwrap().starts_with("usage:"));
    }
}

#[test]
fn current_cli_input_read_failures_use_the_common_report_epoch() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("absent-current-packet.json");
    assert!(!Path::new(&missing).exists());
    for binary in [
        env!("CARGO_BIN_EXE_ordinary-trigger"),
        env!("CARGO_BIN_EXE_resolution-values"),
    ] {
        let report = invoke(
            binary,
            &[
                "--inputs",
                missing.to_str().unwrap(),
                "--case",
                "unread-input",
            ],
        );
        assert!(
            report["failure"]
                .as_str()
                .unwrap()
                .starts_with("input read failed:")
        );
    }
}

#[test]
fn historical_packet9_is_rejected_by_parser_and_current_cli_source_guards() {
    let directory = tempfile::tempdir().unwrap();
    let historical = directory
        .path()
        .join("synthetic-historical-epoch-control.json");
    // This is an epoch rejection control, not a source-qualified input reissue.
    let bytes = br#"{"schema_version":9,"kind":"deterministic-input-only-producer-recipes","no_expected_outputs":true,"definitions":{},"fixtures":[]}"#;
    assert_eq!(
        spellwake_reference_tools::packet::Packet::parse(bytes).unwrap_err(),
        "unsupported input packet identity"
    );
    std::fs::write(&historical, bytes).unwrap();
    for binary in [
        env!("CARGO_BIN_EXE_ordinary-trigger"),
        env!("CARGO_BIN_EXE_resolution-values"),
    ] {
        let report = invoke(
            binary,
            &[
                "--inputs",
                historical.to_str().unwrap(),
                "--case",
                "epoch-only",
            ],
        );
        // The selected CLI's byte pin deliberately rejects this synthetic input
        // before the parser. Preserve both guards instead of bypassing the pin.
        assert_eq!(
            report["failure"],
            "input bytes do not match the selected current-schema stage snapshot"
        );
        assert_eq!(report["input_source"]["unchanged"], true);
        assert_eq!(std::fs::read(&historical).unwrap(), bytes);
    }
}
