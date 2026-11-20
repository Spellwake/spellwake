//! Configuration failures remain ordinary executable errors, without panic or fallback.
use std::process::{Command, Output};

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ordinary-game"));
    command.env_remove("SPELLWAKE_CREATURE_VOCABULARY_INPUT");
    command
}

fn assert_configuration_error(output: Output, reason: &str) {
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("invalid qualification input:"), "{stderr}");
    assert!(stderr.contains(reason), "{stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(output.stdout.is_empty());
}

#[test]
fn missing_vocabulary_is_reported_without_panicking() {
    assert_configuration_error(command().output().unwrap(), "qualification requires");
}

#[test]
fn missing_cli_value_is_reported_without_panicking() {
    assert_configuration_error(
        command()
            .arg("--creature-vocabulary-input")
            .output()
            .unwrap(),
        "requires a path",
    );
}

#[test]
fn conflicting_sources_are_reported_without_panicking() {
    assert_configuration_error(
        command()
            .env("SPELLWAKE_CREATURE_VOCABULARY_INPUT", "environment.json")
            .args(["--creature-vocabulary-input", "cli.json"])
            .output()
            .unwrap(),
        "conflicting explicit vocabulary inputs",
    );
}

#[test]
fn unreadable_vocabulary_is_reported_without_panicking() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.json");
    assert_configuration_error(
        command()
            .arg("--creature-vocabulary-input")
            .arg(&missing)
            .output()
            .unwrap(),
        missing.to_str().unwrap(),
    );
}

#[test]
fn malformed_vocabulary_is_reported_without_panicking() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"not JSON").unwrap();
    assert_configuration_error(
        command()
            .arg("--creature-vocabulary-input")
            .arg(file.path())
            .output()
            .unwrap(),
        "expected",
    );
}
