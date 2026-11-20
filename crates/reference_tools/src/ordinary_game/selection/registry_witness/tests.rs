//! Witness/admission controls only. Genuine source owner, synthetic recorder
//! receipt metadata: no native game, publication/image or envelope authority.
use super::*;
use crate::current_profile::{
    CaptureScope, Recorder, RegistryWitness, SourceRef, envelope, validate_registry_with_owner,
    validate_selection_registry_with_owner,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::OnceLock};

fn path(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine input required: {key}"))
}
fn source() -> &'static SelectionSource {
    static SOURCE: OnceLock<SelectionSource> = OnceLock::new();
    SOURCE.get_or_init(|| {
        SelectionSource::load(
            &path("SPELLWAKE_CATALOGUE_METADATA"),
            &path("SPELLWAKE_CATALOGUE_INPUT"),
            crate::creature_types::selected_vocabulary().unwrap(),
        )
        .unwrap()
    })
}
fn recorder(scope: CaptureScope, witness: RegistryWitness<'_>) -> Result<Recorder, String> {
    let source_ref = |path: &str| SourceRef {
        path: path.into(),
        sha256: "0".repeat(64),
        bytes: 0,
    };
    Recorder::new_scoped(
        scope,
        source().vocabulary(),
        source_ref("synthetic-witness-input"),
        source_ref("synthetic-witness-publication"),
        vec![],
        witness,
    )
}
#[test]
fn raw_dto_without_native_owner_cannot_supply_source9_authority() {
    assert_eq!(
        validate_selection_registry_with_owner(&json!({}), "untrusted-id", None).unwrap_err(),
        "Registry9 witness requires its immutable authenticated source owner"
    );
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary; witness unit only"]
fn source9_owner_admits_only_selected_scope_and_exact_native_witness() {
    let source = source();
    let coverage = source.native_coverage().unwrap();
    let mut selected = recorder(
        CaptureScope::ResolutionSelection11,
        RegistryWitness::ResolutionSelection(source),
    )
    .unwrap();
    selected
        .finish_coverage(vec![], vec![], vec![], vec![])
        .unwrap();
    let witness = selected.witness(&json!({})).unwrap();
    assert_eq!(
        witness["schema"],
        "spellwake.current-type-context-witness.v9"
    );
    assert_eq!(
        witness["registry"],
        json!({"tag":"Registry","coverage":coverage})
    );
    assert!(
        witness["caller_delivery"]["calls"]
            .as_object()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        envelope("resolution-selection", json!({}), &selected).unwrap_err(),
        "selection11 ordinary/native envelope admission is not implemented"
    );
    assert_eq!(
        recorder(
            CaptureScope::Current10,
            RegistryWitness::ResolutionSelection(source)
        )
        .err()
        .unwrap(),
        "source9 witness requires the explicit selection11 scope"
    );
    assert!(
        validate_registry_with_owner(&coverage, &source.vocabulary().id().to_string(), None)
            .is_err()
    );
    assert!(
        recorder(
            CaptureScope::Current10,
            RegistryWitness::Registry(&coverage)
        )
        .is_err()
    );
    assert!(
        recorder(
            CaptureScope::ResolutionSelection11,
            RegistryWitness::Registry(&coverage)
        )
        .is_err()
    );
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary; witness corruption controls"]
fn source9_owner_rejects_each_changed_coverage_value_and_count_preserving_key_swap() {
    let source = source();
    let id = source.vocabulary().id().to_string();
    let valid = source.native_coverage().unwrap();
    validate_selection_registry_with_owner(&valid, &id, Some(source)).unwrap();
    for (pointer, replacement) in [
        ("/registryVersion", json!(8)),
        ("/registryVersion", json!(10)),
        ("/registryVersion", json!("9")),
        ("/sourceSha256", json!("0".repeat(64))),
        ("/sourceRecords", json!(9)),
        ("/requiredRecords", json!(8)),
        ("/requiredPartialBindings", json!(8)),
        ("/requiredWithoutBinding", json!(1)),
        ("/fullyQualifiedCards", json!(1)),
        ("/creatureVocabularyId", json!("0".repeat(64))),
        ("/behaviorEvidence", json!("qualified")),
        ("/cards/0/name", json!("Foreign face")),
        (
            "/cards/0/oracleId",
            json!("00000000-0000-0000-0000-000000000000"),
        ),
        ("/cards/0/required", json!(false)),
        ("/cards/0/binding/status", json!("missing")),
        ("/cards/0/binding/program", json!("foreign-program")),
        (
            "/cards/0/binding/scope",
            json!("optional-payment-ordinary-card-v1"),
        ),
        ("/cards/0/binding/semanticSha256", json!("0".repeat(64))),
    ] {
        let mut changed = valid.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert_eq!(
            validate_selection_registry_with_owner(&changed, &id, Some(source)).unwrap_err(),
            "Registry9 witness differs from its immutable authenticated source owner",
            "{pointer}"
        );
    }
    for pointer in ["", "/cards/0", "/cards/0/binding"] {
        let mut changed = valid.clone();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unlisted".into(), json!(true));
        assert!(validate_selection_registry_with_owner(&changed, &id, Some(source)).is_err());
    }
    let mut missing = valid.clone();
    missing["cards"].as_array_mut().unwrap().pop();
    assert!(validate_selection_registry_with_owner(&missing, &id, Some(source)).is_err());
    let mut duplicate = valid.clone();
    duplicate["cards"][1] = duplicate["cards"][0].clone();
    assert!(validate_selection_registry_with_owner(&duplicate, &id, Some(source)).is_err());
    let other = valid["cards"]
        .as_array()
        .unwrap()
        .iter()
        .position(|card| card["binding"]["program"] != valid["cards"][0]["binding"]["program"])
        .unwrap();
    let counts = |value: &Value| {
        let mut counts = std::collections::BTreeMap::<String, usize>::new();
        for card in value["cards"].as_array().unwrap() {
            *counts
                .entry(card["binding"]["program"].as_str().unwrap().into())
                .or_default() += 1;
        }
        counts
    };
    let mut swapped = valid.clone();
    let first = swapped["cards"][0]["binding"]["program"].clone();
    swapped["cards"][0]["binding"]["program"] =
        swapped["cards"][other]["binding"]["program"].clone();
    swapped["cards"][other]["binding"]["program"] = first;
    assert_eq!(counts(&swapped), counts(&valid));
    assert!(validate_selection_registry_with_owner(&swapped, &id, Some(source)).is_err());
    validate_selection_registry_with_owner(&valid, &id, Some(source)).unwrap();
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary; source relationship controls"]
fn source9_witness_requires_its_owned_native_vocabulary_and_old_no_registry_stays_exact() {
    let source = source();
    let coverage = source.native_coverage().unwrap();
    assert_eq!(
        validate_selection_registry_with_owner(&coverage, &"0".repeat(64), Some(source))
            .unwrap_err(),
        "selection witness vocabulary mismatch"
    );
    for scope in [CaptureScope::Current10, CaptureScope::ResolutionSelection11] {
        let mut recorder = recorder(scope, RegistryWitness::NotApplicable).unwrap();
        recorder
            .finish_coverage(vec![], vec![], vec![], vec![])
            .unwrap();
        let witness = recorder.witness(&json!({})).unwrap();
        assert_eq!(
            witness["registry"],
            json!({"tag":"NotApplicable","reason":"NoRegistryConstruction"})
        );
        assert_eq!(
            witness["schema"],
            match scope {
                CaptureScope::Current10 => "spellwake.current-type-context-witness.v8",
                CaptureScope::ResolutionSelection11 => "spellwake.current-type-context-witness.v9",
            }
        );
    }
}
