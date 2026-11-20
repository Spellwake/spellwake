//! Whole-file evidence tests; no test reaches a remote source or publishes a slot.
use chrono::{DateTime, Utc};
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use spellwake_application::price_source::{
    BulkArtifact, EvidenceHash, SourceError, extract_retained, hash_file, scan_artifact,
};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

struct Stage(PathBuf);
impl Stage {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("spellwake-price-source-test-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn card(id: Uuid) -> Value {
    json!({"object":"card","id":id,"finishes":["nonfoil","foil"],"prices":{"usd":"1.23456789","usd_foil":null}})
}
fn artifact(stage: &Stage, records: &[Value]) -> PathBuf {
    let lines = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    stage.put("cards.jsonl.gz", &gzip(lines.as_bytes()))
}

fn catalogue_card(id: Uuid) -> Value {
    json!({"object":"card","id":id,"oracle_id":Uuid::new_v4(),"name":"Unambiguous card","set_id":Uuid::new_v4(),"set":"tst","set_name":"Test set","collector_number":"1","lang":"en","rarity":"rare","finishes":["nonfoil","foil"],"layout":"normal","prices":{"usd":"1.23456789","usd_foil":null,"usd_etched":"2.00"},"card_faces":[{"name":"Front"},{"name":"Back"}]})
}
fn duplicate_field(object: &Value, field: &str) -> String {
    format!(
        "{{{}:{},{}",
        json!(field),
        object.get(field).unwrap(),
        &object.to_string()[1..]
    )
}

#[test]
fn consumed_json_rejects_duplicate_root_projection_fields_from_original_bytes() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let record = catalogue_card(id);
    let wanted = HashSet::from([id]);
    let valid = artifact(&stage, std::slice::from_ref(&record));
    assert_eq!(
        scan_artifact(&valid, &wanted).unwrap().cards[&id]["prices"]["usd"],
        "1.23456789"
    );
    for field in [
        "object",
        "id",
        "oracle_id",
        "name",
        "set_id",
        "set",
        "set_name",
        "collector_number",
        "lang",
        "rarity",
        "finishes",
        "layout",
        "prices",
        "card_faces",
    ] {
        let raw = duplicate_field(&record, field) + "\n";
        let path = stage.put("duplicate.gz", &gzip(raw.as_bytes()));
        assert!(
            matches!(
                scan_artifact(&path, &wanted),
                Err(error) if error == SourceError::InvalidArtifact
            ),
            "{field}"
        );
    }
}

#[test]
fn consumed_json_rejects_duplicate_price_and_face_fields_even_after_a_wanted_row() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let record = catalogue_card(id);
    let raw = record.to_string();
    let wanted = HashSet::from([id]);
    for field in ["usd", "usd_foil", "usd_etched"] {
        let duplicate = duplicate_field(&record["prices"], field);
        let raw = raw.replacen(&record["prices"].to_string(), &duplicate, 1) + "\n";
        let path = stage.put("price.gz", &gzip(raw.as_bytes()));
        assert!(
            matches!(
                scan_artifact(&path, &wanted),
                Err(error) if error == SourceError::InvalidArtifact
            ),
            "{field}"
        );
    }
    let duplicate = duplicate_field(&record["card_faces"][0], "name");
    let raw = raw.replacen(&record["card_faces"][0].to_string(), &duplicate, 1) + "\n";
    let path = stage.put("face.gz", &gzip(raw.as_bytes()));
    assert!(matches!(
        scan_artifact(&path, &wanted),
        Err(error) if error == SourceError::InvalidArtifact
    ));

    let other = catalogue_card(Uuid::new_v4());
    let bad = other.to_string().replacen(
        &other["prices"].to_string(),
        &duplicate_field(&other["prices"], "usd"),
        1,
    );
    let raw = format!("{}\n{bad}\n", card(id));
    let path = stage.put("late.gz", &gzip(raw.as_bytes()));
    assert!(matches!(
        scan_artifact(&path, &wanted),
        Err(error) if error == SourceError::InvalidArtifact
    ));
}

#[test]
fn consumed_json_preserves_missing_unsupported_values_and_unconsumed_vendor_extensions() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let wanted = HashSet::from([id]);
    for prices in [Value::Null, json!([]), json!(17), json!("unavailable")] {
        let mut value = card(id);
        value["prices"] = prices;
        value["card_faces"] = json!([null, 17, [], "unsupported"]);
        let path = artifact(&stage, &[value]);
        assert_eq!(
            scan_artifact(&path, &wanted).unwrap().cards[&id]["prices"],
            json!({})
        );
    }
    let mut value = card(id);
    value.as_object_mut().unwrap().remove("prices");
    let path = artifact(&stage, &[value]);
    assert_eq!(
        scan_artifact(&path, &wanted).unwrap().cards[&id]["prices"],
        json!({})
    );
    let encoded = card(id).to_string();
    let raw = format!(
        r#"{{"vendor_tag":1,"vendor_tag":2,"vendor":{{"x":1,"x":2}},{}}}"#,
        &encoded[1..encoded.len() - 1]
    );
    let path = stage.put("extension.gz", &gzip((raw + "\n").as_bytes()));
    assert_eq!(
        scan_artifact(&path, &wanted).unwrap().cards[&id]["prices"]["usd"],
        "1.23456789"
    );
}
#[test]
fn whole_file_verification_preserves_exact_prices_and_only_requested_printings() {
    let stage = Stage::new();
    let wanted = Uuid::new_v4();
    let other = Uuid::new_v4();
    let path = artifact(&stage, &[card(wanted), card(other)]);
    let scanned = scan_artifact(&path, &HashSet::from([wanted])).unwrap();
    assert_eq!(scanned.record_count, 2);
    assert_eq!(scanned.cards.len(), 1);
    let prices = &scanned.cards[&wanted]["prices"];
    assert_eq!(prices["usd"], "1.23456789");
    assert_eq!(prices["usd_foil"], Value::Null);
    assert!(prices.get("usd_etched").is_none());
    let (hash, count) = hash_file(&path).unwrap();
    assert!(hash.as_str().starts_with("sha256:"));
    assert_eq!(hash.as_str().len(), 71);
    assert_eq!(count, fs::metadata(path).unwrap().len());
}
#[test]
fn duplicate_or_corrupt_tail_rejects_even_after_the_wanted_card_was_found() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let wanted = HashSet::from([id]);
    let duplicate = artifact(&stage, &[card(id), card(id)]);
    assert!(matches!(
        scan_artifact(&duplicate, &wanted),
        Err(error) if error == SourceError::InvalidArtifact
    ));
    let valid = gzip((card(id).to_string() + "\n").as_bytes());
    for corrupt in [
        valid[..valid.len() - 4].to_vec(),
        [valid.as_slice(), b"trailing junk"].concat(),
    ] {
        let path = stage.put("corrupt.gz", &corrupt);
        assert!(matches!(
            scan_artifact(&path, &wanted),
            Err(error) if error == SourceError::InvalidArtifact
        ));
    }
    let mut checksum = valid;
    let last = checksum.len() - 8;
    checksum[last] ^= 1;
    assert!(matches!(
        scan_artifact(&stage.put("checksum.gz", &checksum), &wanted),
        Err(error) if error == SourceError::InvalidArtifact
    ));
}
#[test]
fn all_gzip_members_are_scanned_and_jsonl_shape_is_not_substituted() {
    let stage = Stage::new();
    let one = Uuid::new_v4();
    let two = Uuid::new_v4();
    let members = [
        gzip(card(one).to_string().as_bytes()),
        gzip(("\n".to_owned() + &card(two).to_string() + "\n").as_bytes()),
    ]
    .concat();
    assert_eq!(
        scan_artifact(&stage.put("members.gz", &members), &HashSet::new())
            .unwrap()
            .record_count,
        2
    );
    for raw in [
        b"[]".as_slice(),
        b"\n",
        b"not-json",
        b"",
        b"{\"object\":\"card\",\"id\":\"bad\"}",
    ] {
        assert!(scan_artifact(&stage.put("invalid.gz", &gzip(raw)), &HashSet::new()).is_err());
    }
    let large = stage.put("oversize.gz", &gzip(&vec![b'x'; 262_145]));
    assert!(matches!(
        scan_artifact(&large, &HashSet::new()),
        Err(SourceError::BoundsExceeded)
    ));
}
#[test]
fn malformed_price_remains_unusable_data_while_identity_corruption_rejects_the_batch() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let mut value = card(id);
    value["prices"]["usd"] = json!(1.2);
    let path = artifact(&stage, &[value.clone()]);
    assert_eq!(
        scan_artifact(&path, &HashSet::from([id])).unwrap().cards[&id]["prices"]["usd"],
        json!(1.2)
    );
    for identity in [Uuid::nil().to_string(), id.to_string().to_uppercase()] {
        value["id"] = json!(identity);
        assert!(scan_artifact(&artifact(&stage, &[value.clone()]), &HashSet::new()).is_err());
    }
}
fn metadata() -> Value {
    json!({"object":"bulk_data","id":"922288cb-4bef-45e1-bb30-0c2bd3d3534f","type":"all_cards","updated_at":"2026-11-01T12:00:00Z","jsonl_download_uri":"https://data.scryfall.io/all-cards/all-cards-20261101120000.jsonl.gz","compressed_size":123})
}
fn time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value)
        .unwrap()
        .with_timezone(&Utc)
}
#[test]
fn only_current_exact_all_cards_jsonl_metadata_is_eligible() {
    let value = metadata();
    let parsed: BulkArtifact = serde_json::from_value(value.clone()).unwrap();
    parsed.validate(time("2026-11-02T12:00:00Z")).unwrap();
    assert_eq!(
        parsed.validate(time("2026-11-02T12:00:00.001Z")),
        Err(SourceError::StaleArtifact)
    );
    assert_eq!(
        parsed.validate(time("2026-11-01T11:59:59Z")),
        Err(SourceError::StaleArtifact)
    );
    for (field, bad) in [
        ("id", json!(Uuid::new_v4())),
        ("type", json!("oracle_cards")),
        (
            "jsonl_download_uri",
            json!("https://data.scryfall.io/all-cards/all-cards-20261101120000.json.gz"),
        ),
        (
            "jsonl_download_uri",
            json!("https://data.scryfall.io.evil.test/all-cards/all-cards-20261101120000.jsonl.gz"),
        ),
        (
            "jsonl_download_uri",
            json!("https://data.scryfall.io/all-cards/all-cards-20261101120000.jsonl.gz?other=1"),
        ),
        ("compressed_size", json!(0)),
    ] {
        let mut changed = value.clone();
        changed[field] = bad;
        assert_eq!(
            serde_json::from_value::<BulkArtifact>(changed)
                .unwrap()
                .validate(time("2026-11-01T12:00:00Z")),
            Err(SourceError::InvalidIndex)
        );
    }
}
#[test]
fn compressed_file_bound_is_checked_before_decompression() {
    let stage = Stage::new();
    let path = stage.0.join("sparse.gz");
    let file = fs::File::create(&path).unwrap();
    file.set_len(2 * 1024 * 1024 * 1024 + 1).unwrap();
    assert!(matches!(
        scan_artifact(Path::new(&path), &HashSet::new()),
        Err(SourceError::BoundsExceeded)
    ));
}

#[test]
fn empty_finishes_are_preserved_without_inventing_a_physical_price_contract() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let mut value = card(id);
    // Observed in the retained all-cards export: an empty finish list is valid
    // source data. Publication still requires the chosen physical finish.
    value["finishes"] = json!([]);
    let scan = scan_artifact(&artifact(&stage, &[value]), &HashSet::from([id])).unwrap();
    assert_eq!(scan.cards[&id]["finishes"], json!([]));
}
#[test]
fn extraction_is_bound_to_exact_manifest_hash_count_and_complete_compressed_stream() {
    let stage = Stage::new();
    let id = Uuid::new_v4();
    let path = artifact(&stage, &[card(id)]);
    let scan = scan_artifact(&path, &HashSet::new()).unwrap();
    let expected = hash_file(&path).unwrap().0;
    assert_eq!(scan.hash, expected);
    extract_retained(&path, &expected, 1, &HashSet::from([id])).unwrap();
    assert!(matches!(
        extract_retained(&path, &expected, 2, &HashSet::new()),
        Err(SourceError::HashMismatch)
    ));
    let wrong = EvidenceHash::try_from(format!("sha256:{}", "0".repeat(64))).unwrap();
    assert!(matches!(
        extract_retained(&path, &wrong, 1, &HashSet::new()),
        Err(SourceError::HashMismatch)
    ));
    for invalid in [
        "".to_owned(),
        "bad".to_owned(),
        format!("sha256:{}", "X".repeat(64)),
        format!("sha256:{}", "é".repeat(32)),
    ] {
        assert!(EvidenceHash::try_from(invalid).is_err());
    }
}
