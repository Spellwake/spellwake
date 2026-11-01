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
