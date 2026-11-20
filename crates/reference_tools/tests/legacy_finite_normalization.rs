//! Actual old source19 public-report bytes across structural normalization.
//! Source-dependent tests are opt-in. These are isolated route-preservation
//! checks, not historical-image, card, format or capture qualification.
use sha2::{Digest, Sha256};
use spellwake_reference_tools::ordinary_game::{
    mandatory::{MandatoryCase, MandatorySource},
    optional::{OptionalCase, OptionalSource},
};
use std::{io::Write, path::PathBuf};

fn required(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit {name} required"))
}
fn retain(name: &str, bytes: &[u8]) -> String {
    let directory = required("SPELLWAKE_LEGACY_FINITE_OUTPUT");
    std::fs::create_dir_all(&directory).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))
        .unwrap();
    file.write_all(bytes).unwrap();
    let digest = format!("{:x}", Sha256::digest(bytes));
    println!("{name}: {} bytes SHA256 {digest}", bytes.len());
    digest
}

#[test]
#[ignore = "requires genuine complete catalogue/metadata/vocabulary and fresh explicit output directory; old-route byte preservation only"]
fn all_mandatory_source19_reports_preserve_actual_pre_normalization_bytes() {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary().unwrap();
    let source = MandatorySource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocabulary,
    )
    .unwrap();
    let report = source.run(&MandatoryCase::ALL, vocabulary).unwrap();
    let bytes = serde_json::to_vec(&report).unwrap();
    let digest = retain("mandatory-source19.json", &bytes);
    assert_eq!(report.runs.len(), 23);
    assert!(
        report.completed(),
        "actual public report contains a failed old run"
    );
    assert_eq!(
        digest,
        "6096786cbdecc674d2d3bb1a0d18bfdbf7db6f7e962cf68a5a60327805f070a6"
    );
}

#[test]
#[ignore = "requires genuine complete catalogue/metadata/vocabulary and fresh explicit output directory; old-route byte preservation only"]
fn all_optional_source19_reports_preserve_actual_pre_normalization_bytes() {
    let vocabulary = spellwake_reference_tools::creature_types::selected_vocabulary().unwrap();
    let source = OptionalSource::load(
        &required("SPELLWAKE_CATALOGUE_METADATA"),
        &required("SPELLWAKE_CATALOGUE_INPUT"),
        &vocabulary,
    )
    .unwrap();
    let report = source.run(&OptionalCase::ALL, vocabulary).unwrap();
    let bytes = serde_json::to_vec(&report).unwrap();
    let digest = retain("optional-source19.json", &bytes);
    assert_eq!(report.runs.len(), 26);
    assert!(
        report.completed(),
        "actual public report contains a failed old run"
    );
    assert_eq!(
        digest,
        "7d796ac8de81f5b26e3f25bce5d4b19d5c52d8dd5d8d001ca6895908bc93ed89"
    );
}
