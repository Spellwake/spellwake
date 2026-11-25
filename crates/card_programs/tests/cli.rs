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

#[test]
fn cli_support_report_tracks_incremental_cache_and_isolates_preview_formats() {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;

    let binary = env!("CARGO_BIN_EXE_spellwake-card-programs");
    let root = std::env::temp_dir().join(format!(
        "spellwake-support-report-cli-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let vocabulary = spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
        "synthetic-support-report-vocab".into(),
        [0; 32],
        vec!["Bear".into(), "Cat".into(), "Elf".into()],
    )
    .unwrap();
    let vocab_path = root.join("vocab.json");
    std::fs::write(
        &vocab_path,
        serde_json::to_vec(&serde_json::json!({
            "rules_revision": vocabulary.rules_revision(),
            "rules_sha256": "0".repeat(64),
            "members": vocabulary.members(),
            "id": vocabulary.id().to_string()
        }))
        .unwrap(),
    )
    .unwrap();

    let ordinary_card = serde_json::json!({
        "object": "card",
        "id": "ord-1",
        "oracle_id": "11111111-1111-1111-1111-111111111111",
        "name": "Grizzly Bears",
        "layout": "normal",
        "type_line": "Creature \u{2014} Bear",
        "mana_cost": "{1}{G}",
        "power": "2",
        "toughness": "2",
        "oracle_text": "",
        "legalities": {
            "commander": "legal",
            "standard": "not_legal",
            "pioneer": "not_legal",
            "modern": "legal",
            "legacy": "legal",
            "vintage": "legal",
            "future": "not_legal"
        }
    });
    let preview_only_card = serde_json::json!({
        "object": "card",
        "id": "prev-1",
        "oracle_id": "22222222-2222-2222-2222-222222222222",
        "name": "Preview Elf",
        "layout": "normal",
        "type_line": "Creature \u{2014} Elf",
        "mana_cost": "{1}{G}",
        "power": "1",
        "toughness": "1",
        "oracle_text": "When Preview Elf enters, draw a card.",
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

    let snap_path = root.join("oracle.jsonl.gz");
    let meta_path = root.join("oracle.meta.json");
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    writeln!(gz, "{ordinary_card}").unwrap();
    writeln!(gz, "{preview_only_card}").unwrap();
    let compressed = gz.finish().unwrap();
    std::fs::write(&snap_path, &compressed).unwrap();
    std::fs::write(
        &meta_path,
        serde_json::json!({
            "object": "bulk_data",
            "id": "27bf3214-1271-490b-bdfe-c0be6c23d02e",
            "type": "oracle_cards",
            "updated_at": "2026-10-29T21:00:00Z",
            "jsonl_download_uri": "https://data.scryfall.io/oracle-cards/fixture.jsonl.gz",
            "compressed_size": compressed.len()
        })
        .to_string(),
    )
    .unwrap();

    let state_path = root.join("state.json");
    let cache_path = root.join("cache.json");

    let out1 = Command::new(binary)
        .args([
            "support-report",
            meta_path.to_str().unwrap(),
            snap_path.to_str().unwrap(),
            "--creature-vocabulary-input",
            vocab_path.to_str().unwrap(),
            "--write-state",
            state_path.to_str().unwrap(),
            "--write-cache",
            cache_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out1.status.success(),
        "{}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let r1: serde_json::Value = serde_json::from_slice(&out1.stdout).unwrap();
    assert_eq!(r1["status"], "success");
    assert_eq!(r1["data"]["ordinaryBaseFormats"]["requiredRecords"], 1);
    assert_eq!(r1["data"]["ordinaryBaseFormats"]["stage1AstParsed"], 1);
    assert_eq!(r1["data"]["ordinaryBaseFormats"]["stage2EngineBound"], 1);
    assert_eq!(r1["data"]["ordinaryBaseFormats"]["stage3GameQualified"], 0);
    assert_eq!(
        r1["data"]["previewFormats"]["isolatedFromOrdinaryLegality"],
        true
    );
    assert_eq!(r1["data"]["previewFormats"]["previewOnlyRecords"], 1);
    assert_eq!(r1["data"]["incremental"]["recompiledCards"], 2);
    assert_eq!(r1["data"]["incremental"]["reusedCachedCards"], 0);

    let out2 = Command::new(binary)
        .args([
            "support-report",
            meta_path.to_str().unwrap(),
            snap_path.to_str().unwrap(),
            "--creature-vocabulary-input",
            vocab_path.to_str().unwrap(),
            "--previous-state",
            state_path.to_str().unwrap(),
            "--cache",
            cache_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let r2: serde_json::Value = serde_json::from_slice(&out2.stdout).unwrap();
    assert_eq!(
        r2["data"]["incremental"]["diff"]["refreshStatus"],
        "unchanged"
    );
    assert_eq!(r2["data"]["incremental"]["reusedCachedCards"], 2);
    assert_eq!(r2["data"]["incremental"]["recompiledCards"], 0);
    assert_eq!(
        r2["data"]["ordinaryBaseFormats"],
        r1["data"]["ordinaryBaseFormats"]
    );
    assert_eq!(r2["data"]["previewFormats"], r1["data"]["previewFormats"]);

    std::fs::remove_dir_all(&root).unwrap();
}
