//! Synthetic stage-boundary tests, never production catalogue qualification.
use serde_json::{Value, json};
use spellwake_application::{
    catalogue_source::stage_catalogue,
    price_source::{hash_file, scan_artifact},
};
use std::{collections::HashSet, fs, path::PathBuf};
use uuid::Uuid;

#[path = "support/catalogue.rs"]
mod source_fixture;
use source_fixture::{Fixture, card, fixture, gzip};
fn markers(fixture: &Fixture) -> Vec<PathBuf> {
    fs::read_dir(&fixture.root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("complete.json"))
        .filter(|path| path.is_file())
        .collect()
}

#[test]
fn metadata_stage_preserves_finishes_faces_and_source_labels_without_price_projection_leakage() {
    let oracle = Uuid::new_v4();
    let set = Uuid::new_v4();
    let first = card(oracle, set);
    let mut translated = card(oracle, set);
    translated["lang"] = json!("ja");
    translated["card_faces"] = json!([{"name":"Front"},{"name":"Back"}]);
    let fixture = fixture(&[first, translated]);
    let (marker, stage) = stage_catalogue(&fixture.qualification, &fixture.root).unwrap();
    assert!(marker.is_file());
    assert_eq!(stage.supported_records, 2);
    assert_eq!(stage.unresolved_records, 0);
    assert_eq!(stage.printing_count, 4);
    assert_eq!(stage.definition_count, 1);
    assert_eq!(stage.set_count, 1);
    let records = fs::read_to_string(&stage.records_path).unwrap();
    let rows: Vec<Value> = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows[0]["metadata"]["collectorNumber"], "123a★/b");
    assert_eq!(rows[0]["metadata"]["finishes"], json!(["foil", "nonfoil"]));
    assert_eq!(rows[1]["metadata"]["faceNames"], json!(["Front", "Back"]));
    assert_eq!(rows[1]["metadata"]["language"], "ja");
    assert!(!records.contains("prices"));
    assert_eq!(
        hash_file(&stage.records_path).unwrap().0.as_str(),
        stage.records_hash.as_str()
    );
    let old = scan_artifact(&fixture.artifact, &HashSet::new()).unwrap();
    assert_eq!(old.record_count, 2);
}

#[test]
fn unrepresentable_oracle_and_unknown_or_duplicate_finishes_are_retained_as_explicit_unresolved_records()
 {
    let mut reversible = card(Uuid::new_v4(), Uuid::new_v4());
    reversible.as_object_mut().unwrap().remove("oracle_id");
    reversible["card_faces"] = json!([{"oracle_id":Uuid::new_v4(),"name":"Front"},{"oracle_id":Uuid::new_v4(),"name":"Back"}]);
    let mut unknown = card(Uuid::new_v4(), Uuid::new_v4());
    unknown["finishes"] = json!(["future_finish"]);
    let mut duplicate = card(Uuid::new_v4(), Uuid::new_v4());
    duplicate["finishes"] = json!(["foil", "foil"]);
    let fixture = fixture(&[reversible, unknown, duplicate]);
    let (_, stage) = stage_catalogue(&fixture.qualification, &fixture.root).unwrap();
    assert_eq!(stage.supported_records, 0);
    assert_eq!(stage.unresolved_records, 3);
    assert_eq!(stage.printing_count, 0);
    for reason in [
        "missing_root_oracle",
        "unsupported_finish",
        "duplicate_finish",
    ] {
        assert_eq!(stage.unresolved_reasons.get(reason), Some(&1));
    }
    assert_eq!(
        fs::read_to_string(stage.records_path)
            .unwrap()
            .lines()
            .count(),
        3
    );
}

#[test]
fn changed_source_counts_metadata_or_corrupt_tail_never_create_a_complete_stage() {
    for kind in ["count", "metadata", "tail", "duplicate", "hash"] {
        let record = card(Uuid::new_v4(), Uuid::new_v4());
        let fixture = fixture(std::slice::from_ref(&record));
        let mut qualified: Value =
            serde_json::from_slice(&fs::read(&fixture.qualification).unwrap()).unwrap();
        match kind {
            "count" => {
                qualified["recordCount"] = json!(2);
                fs::write(&fixture.qualification, qualified.to_string()).unwrap();
            }
            "metadata" => {
                fs::write(fixture.root.join("bulk-index.json"), b"{}").unwrap();
            }
            "tail" => {
                let mut bytes = fs::read(&fixture.artifact).unwrap();
                bytes.pop();
                fs::write(&fixture.artifact, bytes).unwrap();
            }
            "duplicate" => fs::write(&fixture.artifact, gzip(&[record.clone(), record])).unwrap(),
            "hash" => {
                qualified["hash"] = json!(format!("sha256:{}", "0".repeat(64)));
                fs::write(&fixture.qualification, qualified.to_string()).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            stage_catalogue(&fixture.qualification, &fixture.root).is_err(),
            "{kind}"
        );
        assert!(markers(&fixture).is_empty(), "{kind}");
    }
}
