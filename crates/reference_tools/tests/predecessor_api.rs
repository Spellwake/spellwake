//! Opt-in whole typed API byte preservation against the actual B03 captures.
use serde::Serialize;
use sha2::{Digest, Sha256};
use spellwake_contracts::ApiResponse;
use spellwake_reference_tools::{
    creature_types, current_capture,
    ordinary_game::{
        fixture::CaseId,
        interaction::{InteractionCase, InteractionSource},
        run_cases,
        teaching::{TeachingCase, TeachingSource},
    },
};
use std::{fs, path::PathBuf};

fn baseline(name: &str, expected_sha256: &str) -> Vec<u8> {
    let directory = PathBuf::from(
        std::env::var_os("SPELLWAKE_PREDECESSOR_API_BASELINE_DIR")
            .expect("explicit retained B03 API directory"),
    );
    let bytes = fs::read(directory.join(name)).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        expected_sha256,
        "baseline must be the actual frozen pre-edit typed API bytes"
    );
    bytes
}
fn preserve<T: Serialize>(name: &str, expected: Vec<u8>, report: &T, completed: bool) {
    assert!(
        completed,
        "predecessor trajectory must complete before byte comparison"
    );
    let mut actual = serde_json::to_vec(&ApiResponse::<_, ()>::Success { data: report }).unwrap();
    actual.push(b'\n');
    if let Some(directory) = std::env::var_os("SPELLWAKE_PREDECESSOR_API_OBSERVED_DIR") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(name), &actual).unwrap();
    }
    let first_difference = actual.iter().zip(&expected).position(|(a, b)| a != b);
    assert!(
        actual == expected,
        "whole typed API bytes differ for {name}: actual {} expected {} bytes; first differing byte {first_difference:?}",
        actual.len(),
        expected.len()
    );
}
#[test]
#[ignore = "requires actual frozen B03 API bytes and source-qualified vocabulary"]
fn historical_game_api_bytes_are_preserved() {
    let expected = baseline(
        "historical-all.api.json",
        "baa07079e96a7efabd58c81877f767b955521b3b349f4b1a53fea3e76c7507c9",
    );
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    current_capture::without_capture(|| {
        let report = run_cases(&CaseId::ALL, vocabulary);
        preserve(
            "historical-all.api.json",
            expected,
            &report,
            report.completed(),
        );
    });
}
#[test]
#[ignore = "requires actual frozen B03 API bytes, pinned catalogue and source-qualified vocabulary"]
fn teaching_and_interaction_api_bytes_are_preserved() {
    let teaching_expected = baseline(
        "teaching-all.api.json",
        "561d8ad9a6a5cd44b84d89197a09a5da9c7312e7ddd8ee0280eb053bd244ce16",
    );
    let interaction_expected = baseline(
        "interaction-all.api.json",
        "22bd625f8a3a40cc590fa27d913db96d9ad9a2f789186646ab157701d5cfc889",
    );
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    // Supply the same absolute source paths used for the B03 typed response.
    let directory = PathBuf::from(
        std::env::var_os("SPELLWAKE_MANDATORY_CATALOGUE_DIR")
            .expect("explicit actual pinned catalogue directory"),
    );
    let metadata = directory.join("oracle-metadata.json");
    let snapshot = directory.join("oracle-cards-20261029210158.jsonl.gz");
    current_capture::without_capture(|| {
        let teaching = TeachingSource::load(&metadata, &snapshot, &vocabulary)
            .unwrap()
            .run(&TeachingCase::ALL, vocabulary.clone())
            .unwrap();
        preserve(
            "teaching-all.api.json",
            teaching_expected,
            &teaching,
            teaching.completed(),
        );
        let interaction = InteractionSource::load(&metadata, &snapshot, &vocabulary)
            .unwrap()
            .run(&InteractionCase::ALL, vocabulary)
            .unwrap();
        preserve(
            "interaction-all.api.json",
            interaction_expected,
            &interaction,
            interaction.completed(),
        );
    });
}
