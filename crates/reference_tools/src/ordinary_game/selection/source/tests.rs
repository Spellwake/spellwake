use super::*;
use serde_json::{Value, json};
use std::{path::PathBuf, sync::OnceLock};

fn raw_descriptor() -> Value {
    packet::unique_json(DEFINITIONS).unwrap()
}

fn parse(value: &Value) -> Result<SourceDefinitions, String> {
    descriptor(&serde_json::to_vec(value).unwrap())
}

#[test]
fn compiled_descriptor_has_closed_nine_input_rows_only() {
    // Grammar evidence only; this default test cannot qualify a real source.
    let definitions = descriptor(DEFINITIONS).unwrap();
    assert_eq!(definitions.definitions.len(), 9);
    assert!(
        definitions
            .definitions
            .windows(2)
            .all(|r| r[0].oracle_id < r[1].oracle_id)
    );
    assert_eq!(definitions.registry_version, 9);
    assert_eq!(definitions.profile, "resolution-sacrifice-discard-v1");
    assert!(
        definitions
            .definitions
            .iter()
            .all(|row| row.language == "en")
    );
}

#[test]
fn descriptor_rejects_unknown_missing_duplicate_and_wrong_typed_fields() {
    let original = raw_descriptor();
    let mut extra = original.clone();
    extra["unexpected"] = json!(true);
    assert!(parse(&extra).err().unwrap().contains("descriptor schema"));
    let mut missing = original.clone();
    missing.as_object_mut().unwrap().remove("definitions");
    assert!(parse(&missing).is_err());
    let mut nested = original.clone();
    nested["definitions"][0]["Program"] = json!({});
    assert!(parse(&nested).is_err());
    let mut scalar = original;
    scalar["definitions"][0]["sourceLineOneBased"] = json!(true);
    assert!(parse(&scalar).is_err());
    assert!(descriptor(br#"{"schema_version":1,"schema_version":1}"#).is_err());
    let nested_duplicate = String::from_utf8(DEFINITIONS.to_vec()).unwrap().replacen(
        "\"name\": \"Diabolic Edict\"",
        "\"name\": \"Diabolic Edict\", \"name\": \"Diabolic Edict\"",
        1,
    );
    assert!(
        descriptor(nested_duplicate.as_bytes())
            .err()
            .unwrap()
            .contains("descriptor JSON")
    );
}

#[test]
fn descriptor_rejects_foreign_header_and_nonunique_source_coordinates() {
    for (key, value) in [
        ("schema_version", json!(2)),
        ("profile", json!("resolution-selection-natural-mana-v1")),
        ("registry_version", json!(10)),
        ("metadata_bytes", json!(564)),
        ("metadata_sha256", json!("0".repeat(64))),
        ("snapshot_bytes", json!(1)),
        ("snapshot_sha256", json!("0".repeat(64))),
        ("source_records", json!(9)),
        ("creature_vocabulary_id", json!("0".repeat(64))),
    ] {
        let mut changed = raw_descriptor();
        changed[key] = value;
        assert_eq!(
            parse(&changed).err().unwrap(),
            "selection descriptor closed source9 header mismatch",
            "{key}"
        );
    }
    for key in ["sourceLineOneBased", "oracleId", "scryfallId"] {
        let mut changed = raw_descriptor();
        changed["definitions"][1][key] = changed["definitions"][0][key].clone();
        assert_eq!(
            parse(&changed).err().unwrap(),
            "selection descriptor duplicate or invalid source coordinate",
            "{key}"
        );
    }
    for number in [0, SOURCE_RECORDS + 1] {
        let mut changed = raw_descriptor();
        changed["definitions"][0]["sourceLineOneBased"] = json!(number);
        assert!(parse(&changed).err().unwrap().contains("source coordinate"));
    }
    let mut fewer = raw_descriptor();
    fewer["definitions"].as_array_mut().unwrap().pop();
    assert!(parse(&fewer).is_err());
}

#[test]
fn unavailable_input_has_no_fallback_source() {
    // Synthetic vocabulary is a negative IO control only, never source evidence.
    let foreign = Arc::new(
        CreatureTypeVocabulary::new("negative-io-control".into(), [0; 32], vec!["Cat".into()])
            .unwrap(),
    );
    let missing = tempfile::tempdir().unwrap();
    let result = SelectionSource::load(
        &missing.path().join("missing-metadata"),
        &missing.path().join("missing-gzip"),
        foreign,
    );
    assert!(result.err().unwrap().starts_with("selection metadata:"));
}

struct Genuine {
    source: SelectionSource,
    metadata: Vec<u8>,
    snapshot: Vec<u8>,
    raw_rows: BTreeMap<String, Vec<u8>>,
}

fn input(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine source input required: {key}"))
}

fn genuine() -> &'static Genuine {
    static SOURCE: OnceLock<Genuine> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let metadata_path = input("SPELLWAKE_CATALOGUE_METADATA");
        let snapshot_path = input("SPELLWAKE_CATALOGUE_INPUT");
        let vocabulary = crate::creature_vocabulary::load_vocabulary(&input(
            "SPELLWAKE_CREATURE_VOCABULARY_INPUT",
        ))
        .unwrap();
        let source = SelectionSource::load(&metadata_path, &snapshot_path, vocabulary).unwrap();
        let metadata = std::fs::read(metadata_path).unwrap();
        let snapshot = std::fs::read(snapshot_path).unwrap();
        let mut reader = BufReader::new(MultiGzDecoder::new(snapshot.as_slice()));
        let mut bytes = Vec::new();
        let mut raw_rows = BTreeMap::new();
        let by_line: BTreeMap<_, _> = source
            .definitions()
            .iter()
            .map(|r| (r.source_line_one_based, &r.oracle_id))
            .collect();
        for number in 1..=SOURCE_RECORDS {
            bytes.clear();
            assert_ne!(reader.read_until(b'\n', &mut bytes).unwrap(), 0);
            if let Some(oracle) = by_line.get(&number) {
                raw_rows.insert((*oracle).clone(), bytes.clone());
            }
        }
        assert_eq!(raw_rows.len(), 9);
        Genuine {
            source,
            metadata,
            snapshot,
            raw_rows,
        }
    })
}

#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn actual_full_source_load_has_exact_nine_partial_native_bindings() {
    let source = &genuine().source;
    source.validate_witness(TEACHING_VOCABULARY_ID).unwrap();
    assert_eq!(source.inventory().cards.len(), 39_196);
    assert_eq!(source.definitions().len(), 9);
    assert_eq!(source.registry().coverage().required_partial_bindings, 9);
    assert_eq!(source.registry().coverage().fully_qualified_cards, 0);
    assert_eq!(source.registry().coverage().registry_version, 9);
    for row in source.definitions() {
        let id = OracleId::try_from(row.oracle_id.clone()).unwrap();
        assert!(source.registry().resolve(&id).is_some());
        verify_raw_line(
            row,
            row.source_line_one_based,
            &genuine().raw_rows[&row.oracle_id],
        )
        .unwrap();
    }
    assert_eq!(
        source.validate_witness(&"0".repeat(64)).err().unwrap(),
        "selection witness vocabulary mismatch"
    );
}

#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn actual_raw_rows_reject_each_tuple_field_and_exact_coordinate_corruption() {
    for row in genuine().source.definitions() {
        let raw = &genuine().raw_rows[&row.oracle_id];
        for field in ["name", "oracle_id", "id", "set", "collector_number", "lang"] {
            let mut changed = row.clone();
            match field {
                "name" => changed.name.push('!'),
                "oracle_id" => changed.oracle_id = "00000000-0000-0000-0000-000000000000".into(),
                "id" => changed.scryfall_id = "00000000-0000-0000-0000-000000000000".into(),
                "set" => changed.set.push('!'),
                "collector_number" => changed.collector.push('!'),
                "lang" => changed.language = "fr".into(),
                _ => unreachable!(),
            }
            assert_eq!(
                verify_raw_line(&changed, row.source_line_one_based, raw)
                    .err()
                    .unwrap(),
                format!("selection raw source field mismatch: {field}")
            );
        }
        assert_eq!(
            verify_raw_line(row, row.source_line_one_based + 1, raw)
                .err()
                .unwrap(),
            "selection exact raw line/LF hash mismatch"
        );
        let mut hash = row.clone();
        hash.raw_line_sha256_including_lf = "0".repeat(64);
        assert!(verify_raw_line(&hash, row.source_line_one_based, raw).is_err());
        assert!(verify_raw_line(row, row.source_line_one_based, &raw[..raw.len() - 1]).is_err());
    }
    let mut shifted = genuine().source.definitions().to_vec();
    shifted[0].source_line_one_based += 1;
    assert_eq!(
        verify_raw_rows(&genuine().snapshot, &shifted)
            .err()
            .unwrap(),
        "selection exact raw line/LF hash mismatch"
    );
}

#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn actual_pinned_bytes_and_native_vocabulary_refuse_foreign_input() {
    let data = genuine();
    let definitions = descriptor(DEFINITIONS).unwrap();
    authenticate_bytes(
        &data.metadata,
        &data.snapshot,
        &definitions,
        data.source.vocabulary(),
    )
    .unwrap();
    let mut metadata = data.metadata.clone();
    metadata[0] ^= 1;
    assert_eq!(
        authenticate_bytes(
            &metadata,
            &data.snapshot,
            &definitions,
            data.source.vocabulary()
        )
        .err()
        .unwrap(),
        "selection metadata bytes mismatch"
    );
    let mut snapshot = data.snapshot.clone();
    snapshot[0] ^= 1;
    assert_eq!(
        authenticate_bytes(
            &data.metadata,
            &snapshot,
            &definitions,
            data.source.vocabulary()
        )
        .err()
        .unwrap(),
        "selection snapshot bytes mismatch"
    );
    assert!(
        authenticate_bytes(
            &data.metadata,
            &snapshot[..snapshot.len() - 1],
            &definitions,
            data.source.vocabulary()
        )
        .is_err()
    );
    let foreign = CreatureTypeVocabulary::new(
        "synthetic-negative-only".into(),
        [0; 32],
        vec!["Cat".into()],
    )
    .unwrap();
    assert_eq!(
        authenticate_bytes(&data.metadata, &data.snapshot, &definitions, &foreign)
            .err()
            .unwrap(),
        "selection native vocabulary mismatch"
    );
}

#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn public_load_rejects_changed_files_and_foreign_native_vocabulary() {
    let data = genuine();
    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("metadata.json");
    let snapshot = directory.path().join("snapshot.gz");
    let mut changed_metadata = data.metadata.clone();
    changed_metadata[0] ^= 1;
    std::fs::write(&metadata, &changed_metadata).unwrap();
    std::fs::write(&snapshot, &data.snapshot).unwrap();
    assert_eq!(
        SelectionSource::load(&metadata, &snapshot, Arc::clone(data.source.vocabulary()))
            .err()
            .unwrap(),
        "selection metadata bytes mismatch"
    );
    std::fs::write(&metadata, &data.metadata).unwrap();
    let mut changed_snapshot = data.snapshot.clone();
    changed_snapshot[0] ^= 1;
    std::fs::write(&snapshot, changed_snapshot).unwrap();
    assert_eq!(
        SelectionSource::load(&metadata, &snapshot, Arc::clone(data.source.vocabulary()))
            .err()
            .unwrap(),
        "selection snapshot bytes mismatch"
    );
    std::fs::write(&snapshot, &data.snapshot).unwrap();
    let foreign = Arc::new(
        CreatureTypeVocabulary::new(
            "synthetic-negative-only".into(),
            [0; 32],
            vec!["Cat".into()],
        )
        .unwrap(),
    );
    assert_eq!(
        SelectionSource::load(&metadata, &snapshot, foreign)
            .err()
            .unwrap(),
        "selection native vocabulary mismatch"
    );
}

#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn actual_native_source_rows_reject_foreign_binding_identity_printing_and_signature() {
    let source = &genuine().source;
    for field in ["name", "printing", "signature"] {
        let mut rows = source.definitions().to_vec();
        match field {
            "name" => rows[0].name.push('!'),
            "printing" => rows[0].scryfall_id = "00000000-0000-0000-0000-000000000000".into(),
            "signature" => rows[0].registry_signature_v1_sha256 = "0".repeat(64),
            _ => unreachable!(),
        }
        assert_eq!(
            validate_bindings(
                source.registry(),
                source.inventory(),
                source.vocabulary(),
                &rows
            )
            .err()
            .unwrap(),
            "selection exact native source identity/signature mismatch",
            "{field}"
        );
    }
    let mut rows = source.definitions().to_vec();
    rows[0].oracle_id = "00000000-0000-0000-0000-000000000000".into();
    assert_eq!(
        validate_bindings(
            source.registry(),
            source.inventory(),
            source.vocabulary(),
            &rows
        )
        .err()
        .unwrap(),
        "selection native binding missing"
    );
    let mut rows = source.definitions().to_vec();
    rows[8] = rows[0].clone();
    assert_eq!(
        validate_bindings(
            source.registry(),
            source.inventory(),
            source.vocabulary(),
            &rows
        )
        .err()
        .unwrap(),
        "selection duplicate native source identity"
    );
    let mut rows = source.definitions().to_vec();
    rows.pop();
    assert_eq!(
        validate_bindings(
            source.registry(),
            source.inventory(),
            source.vocabulary(),
            &rows
        )
        .err()
        .unwrap(),
        "selection native source9 coverage mismatch"
    );
}
