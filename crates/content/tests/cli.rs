//! Process-level validation of saved-snapshot CLI envelopes and gzip input.
use flate2::{Compression, write::GzEncoder};
use std::{fs, io::Write, process::Command};
fn invoke(args: &[&str]) -> (bool, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_spellwake-content"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}
#[test]
fn cli_inventories_real_gzip_and_separates_input_failures_from_io_errors() {
    let root = std::env::temp_dir().join(format!("spellwake-catalogue-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let meta = root.join("metadata.json");
    let data = root.join("snapshot.gz");
    let card = serde_json::json!({"object":"card","id":"fixture","oracle_id":"fixture-oracle","name":"Fixture","layout":"normal","legalities":{"commander":"legal","standard":"not_legal","pioneer":"not_legal","modern":"not_legal","legacy":"legal","vintage":"restricted"}});
    let mut zip = GzEncoder::new(Vec::new(), Compression::default());
    writeln!(zip, "{card}").unwrap();
    let compressed = zip.finish().unwrap();
    fs::write(&data, &compressed).unwrap();
    let metadata = serde_json::json!({"object":"bulk_data","id":"fixture","type":"oracle_cards","updated_at":"fixture-date","jsonl_download_uri":"https://data.scryfall.io/oracle-cards/fixture.jsonl.gz","compressed_size":compressed.len()});
    fs::write(&meta, metadata.to_string()).unwrap();
    let (ok, report) = invoke(&[meta.to_str().unwrap(), data.to_str().unwrap()]);
    assert!(ok);
    assert_eq!(report["status"], "success");
    assert_eq!(report["data"]["requiredRecords"], 1);
    assert_eq!(report["data"]["qualifiedByThisReport"], 0);
    let (ok, report) = invoke(&[]);
    assert!(!ok);
    assert_eq!(report["status"], "fail");
    fs::write(&meta, "not json").unwrap();
    let (ok, report) = invoke(&[meta.to_str().unwrap(), data.to_str().unwrap()]);
    assert!(!ok);
    assert_eq!(report["status"], "fail");
    let missing = root.join("missing");
    let (ok, report) = invoke(&[missing.to_str().unwrap(), data.to_str().unwrap()]);
    assert!(!ok);
    assert_eq!(report["status"], "error");
    fs::write(&meta, metadata.to_string()).unwrap();
    fs::write(&data, vec![0u8; compressed.len()]).unwrap();
    let (ok, report) = invoke(&[meta.to_str().unwrap(), data.to_str().unwrap()]);
    assert!(!ok);
    assert_eq!(report["status"], "fail");
    fs::remove_dir_all(root).unwrap();
}

const CR_SHA256: &str = "8d860e451f20f38865b725b42d82feb714c725373dd8f3b32b8652b3eeb070ca";

#[test]
fn vocabulary_cli_retains_complete_incomplete_input_and_operational_distinctions() {
    let root = std::env::temp_dir().join(format!("spellwake-creature-cli-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.txt");
    fs::write(&source, [0xff]).unwrap();
    let output = |path: &std::path::Path, revision: &str, digest: &str| {
        Command::new(env!("CARGO_BIN_EXE_spellwake-content"))
            .args([
                "creature-vocabulary",
                path.to_str().unwrap(),
                revision,
                digest,
            ])
            .output()
            .unwrap()
    };
    let unsupported = output(&source, "CR-future", CR_SHA256);
    assert!(unsupported.status.success());
    assert!(unsupported.stderr.is_empty());
    let dto: serde_json::Value = serde_json::from_slice(&unsupported.stdout).unwrap();
    assert_eq!(dto["status"], "success");
    assert_eq!(dto["data"]["tag"], "Incomplete");
    assert_eq!(dto["data"]["incomplete"]["reason"], "UnsupportedSource");
    assert!(dto["data"].get("publication").is_none());
    let mismatch = output(&source, "CR-20260925", CR_SHA256);
    assert!(!mismatch.status.success());
    assert!(!mismatch.stderr.is_empty());
    let dto: serde_json::Value = serde_json::from_slice(&mismatch.stdout).unwrap();
    assert_eq!(dto["status"], "fail");
    assert_eq!(dto["data"]["source"], "source-digest-mismatch");
    let missing = root.join("absent.txt");
    assert!(!missing.exists());
    let unreadable = output(&missing, "CR-20260925", CR_SHA256);
    assert!(!unreadable.status.success());
    assert!(!unreadable.stderr.is_empty());
    let dto: serde_json::Value = serde_json::from_slice(&unreadable.stdout).unwrap();
    assert_eq!(dto["status"], "error");
    assert_eq!(dto["code"], "creature-type-source-read-failed");
    for arguments in [
        vec!["creature-vocabulary"],
        vec![
            "creature-vocabulary",
            source.to_str().unwrap(),
            "CR-20260925",
        ],
        vec![
            "creature-vocabulary",
            source.to_str().unwrap(),
            "CR-20260925",
            "bad-hex",
        ],
        vec![
            "creature-vocabulary",
            source.to_str().unwrap(),
            "CR-20260925",
            "8D860E451F20F38865B725B42D82FEB714C725373DD8F3B32B8652B3EEB070CA",
        ],
        vec![
            "creature-vocabulary",
            source.to_str().unwrap(),
            "",
            CR_SHA256,
        ],
        vec![
            "creature-vocabulary",
            source.to_str().unwrap(),
            "CR-20260925",
            CR_SHA256,
            "extra",
        ],
    ] {
        let bad = Command::new(env!("CARGO_BIN_EXE_spellwake-content"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!bad.status.success());
        assert!(!bad.stderr.is_empty());
        let dto: serde_json::Value = serde_json::from_slice(&bad.stdout).unwrap();
        assert_eq!(dto["status"], "fail");
        assert_eq!(dto["data"]["arguments"], "invalid-input");
    }
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn vocabulary_cli_broken_stdout_reports_stderr_and_failure_without_retry_envelope() {
    use std::{
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::Stdio,
    };
    let root = std::env::temp_dir().join(format!(
        "spellwake-creature-cli-output-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.txt");
    fs::write(&source, "unsupported input").unwrap();
    let (writer, reader) = UnixStream::pair().unwrap();
    // Explicitly disable reads before concurrent CLI tests spawn other children.
    // Closing our handle alone can leave a peer reference alive until exec.
    reader.shutdown(std::net::Shutdown::Both).unwrap();
    drop(reader);
    let fd: OwnedFd = writer.into();
    let result = Command::new(env!("CARGO_BIN_EXE_spellwake-content"))
        .args([
            "creature-vocabulary",
            source.to_str().unwrap(),
            "CR-future",
            CR_SHA256,
        ])
        .stdout(Stdio::from(fd))
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8(result.stderr)
            .unwrap()
            .contains("creature-type publication output failed")
    );
    // Output was directed to the failed socket, so no complete envelope is claimed.
    assert!(result.stdout.is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "actual CLI qualification against explicit retained full CR source; run with --ignored"]
fn vocabulary_cli_complete_uses_the_supported_full_source_pin() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../work/rules/MagicCompRules-20260925.txt");
    let result = Command::new(env!("CARGO_BIN_EXE_spellwake-content"))
        .args([
            "creature-vocabulary",
            source.to_str().unwrap(),
            "CR-20260925",
            CR_SHA256,
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.stderr.is_empty());
    let dto: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(dto["status"], "success");
    assert_eq!(dto["data"]["tag"], "Complete");
    assert_eq!(
        dto["data"]["publication"]["members"]
            .as_array()
            .unwrap()
            .len(),
        324
    );
    assert_eq!(
        dto["data"]["publication"]["identity"]["vocabulary_id"],
        "39721c01eef9fd3f869588b097651bb6115f9cdde5d61bf822d65a8e5435c792"
    );
    assert_eq!(
        dto["data"]["publication"]["section"]["consumed_bytes"],
        "2883"
    );
}

#[test]
fn cli_preview_inventory_and_incremental_diff_isolate_future_legality() {
    let root = std::env::temp_dir().join(format!(
        "spellwake-preview-refresh-cli-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let write_snapshot = |prefix: &str, updated_at: &str, cards: &[serde_json::Value]| {
        let meta = root.join(format!("{prefix}-meta.json"));
        let data = root.join(format!("{prefix}.jsonl.gz"));
        let mut zip = GzEncoder::new(Vec::new(), Compression::default());
        for card in cards {
            writeln!(zip, "{card}").unwrap();
        }
        let compressed = zip.finish().unwrap();
        fs::write(&data, &compressed).unwrap();
        let metadata = serde_json::json!({
            "object": "bulk_data",
            "id": "fixture",
            "type": "oracle_cards",
            "updated_at": updated_at,
            "jsonl_download_uri": format!("https://data.scryfall.io/oracle-cards/{prefix}.jsonl.gz"),
            "compressed_size": compressed.len()
        });
        fs::write(&meta, metadata.to_string()).unwrap();
        (meta, data)
    };

    let spoiler = serde_json::json!({
        "object": "card",
        "id": "spoiler-1",
        "oracle_id": "11111111-1111-1111-1111-111111111111",
        "name": "Spoiler Preview",
        "layout": "normal",
        "type_line": "Instant",
        "mana_cost": "{U}",
        "oracle_text": "Draw a card.",
        "legalities": {
            "commander": "not_legal",
            "standard": "not_legal",
            "pioneer": "not_legal",
            "modern": "not_legal",
            "legacy": "not_legal",
            "vintage": "not_legal",
            "future": "legal"
        }
    });
    let ordinary = serde_json::json!({
        "object": "card",
        "id": "ordinary-1",
        "oracle_id": "22222222-2222-2222-2222-222222222222",
        "name": "Ordinary Bear",
        "layout": "normal",
        "type_line": "Creature — Bear",
        "mana_cost": "{1}{G}",
        "power": "2",
        "toughness": "2",
        "oracle_text": "",
        "legalities": {
            "commander": "legal",
            "standard": "legal",
            "pioneer": "legal",
            "modern": "legal",
            "legacy": "legal",
            "vintage": "legal",
            "future": "legal"
        }
    });

    let (meta1, data1) = write_snapshot(
        "v1",
        "2026-10-29T21:00:00Z",
        &[spoiler.clone(), ordinary.clone()],
    );
    let state_path = root.join("refresh-state.json");

    let (ok, inv) = invoke(&[meta1.to_str().unwrap(), data1.to_str().unwrap()]);
    assert!(ok);
    assert_eq!(inv["data"]["requiredRecords"], 1);
    assert_eq!(inv["data"]["eligibleByFormat"]["standard"], 1);

    let (ok, preview) = invoke(&[
        "preview-inventory",
        meta1.to_str().unwrap(),
        data1.to_str().unwrap(),
    ]);
    assert!(ok);
    assert_eq!(preview["status"], "success");
    assert_eq!(preview["data"]["isolatedFromOrdinaryLegality"], true);
    assert_eq!(preview["data"]["eligibleByFormat"]["future"], 2);
    assert_eq!(preview["data"]["previewOnlyRecords"], 1);
    assert_eq!(preview["data"]["previewNotInStandard"], 1);
    assert_eq!(preview["data"]["ordinaryOverlapRecords"], 1);

    let (ok, diff1) = invoke(&[
        "incremental-diff",
        meta1.to_str().unwrap(),
        data1.to_str().unwrap(),
        "--write-state",
        state_path.to_str().unwrap(),
    ]);
    assert!(ok);
    assert_eq!(diff1["data"]["refreshStatus"], "initial");
    assert_eq!(diff1["data"]["addedOracleIds"].as_array().unwrap().len(), 2);

    let (ok, diff_same) = invoke(&[
        "incremental-diff",
        meta1.to_str().unwrap(),
        data1.to_str().unwrap(),
        "--previous-state",
        state_path.to_str().unwrap(),
    ]);
    assert!(ok);
    assert_eq!(diff_same["data"]["refreshStatus"], "unchanged");
    assert_eq!(diff_same["data"]["unchangedOracleIds"], 2);

    let mut released = spoiler.clone();
    released["legalities"]["standard"] = serde_json::json!("legal");
    let (meta2, data2) = write_snapshot("v2", "2026-11-05T21:00:00Z", &[released, ordinary]);
    let (ok, diff2) = invoke(&[
        "incremental-diff",
        meta2.to_str().unwrap(),
        data2.to_str().unwrap(),
        "--previous-state",
        state_path.to_str().unwrap(),
    ]);
    assert!(ok);
    assert_eq!(diff2["data"]["refreshStatus"], "updated");
    assert_eq!(
        diff2["data"]["ordinaryLegalityChangedOracleIds"],
        serde_json::json!(["11111111-1111-1111-1111-111111111111"])
    );
    assert!(
        diff2["data"]["semanticallyChangedOracleIds"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    fs::remove_dir_all(root).unwrap();
}
