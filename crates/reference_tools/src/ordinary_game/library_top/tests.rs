use super::*;
use std::{path::PathBuf, sync::OnceLock};
fn input(key: &str) -> PathBuf {
    std::env::var_os(key)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine source input required: {key}"))
}
fn genuine() -> &'static LibraryTopSource {
    static SOURCE: OnceLock<LibraryTopSource> = OnceLock::new();
    SOURCE.get_or_init(|| {
        LibraryTopSource::load(
            &input("SPELLWAKE_CATALOGUE_METADATA"),
            &input("SPELLWAKE_CATALOGUE_INPUT"),
            crate::creature_vocabulary::load_vocabulary(&input(
                "SPELLWAKE_CREATURE_VOCABULARY_INPUT",
            ))
            .unwrap(),
        )
        .unwrap()
    })
}
#[test]
fn descriptor_closes_four_sources_without_program_or_capacity_input() {
    let descriptor = authenticated_source::descriptor(DEFINITIONS, Family::LibraryTop).unwrap();
    assert_eq!(descriptor.definitions.len(), 4);
    assert!(authenticated_source::descriptor(DEFINITIONS, Family::Selection).is_err());
    for (field, value) in [
        ("registry_version", serde_json::json!(9)),
        ("source_records", serde_json::json!(4)),
        ("profile", serde_json::json!("other")),
    ] {
        let mut changed: serde_json::Value = serde_json::from_slice(DEFINITIONS).unwrap();
        changed[field] = value;
        assert!(
            authenticated_source::descriptor(
                &serde_json::to_vec(&changed).unwrap(),
                Family::LibraryTop
            )
            .is_err()
        );
    }
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn genuine_full_source_validation_precedes_island_only_physical_subset() {
    let source = genuine();
    let opening = source
        .opening([TeachingDeck::default(); 2], EntropyTape::new(vec![0; 118]))
        .unwrap();
    assert_eq!(source.registry.coverage().cards.len(), 4);
    assert_eq!(opening.definitions.len(), 1);
    assert_eq!(opening.setup.seed.cards.len(), 120);
    assert!(opening.setup.seed.cards.iter().all(|c| matches!(
        c.destination,
        spellwake_engine::object_identity::ObjectDestination::LibraryBottom
    )));
    let mut rows = source.definitions.clone();
    rows[0].registry_signature_v1_sha256 = "0".repeat(64);
    let physical = opening
        .aliases
        .values()
        .map(|a| PhysicalRow {
            alias: a.alias.clone(),
            card: a.card,
            owner: a.owner,
            oracle_id: a.definition_ref.clone(),
            copy: a.alias.rsplit('.').next().unwrap().parse().unwrap(),
        })
        .collect::<Vec<_>>();
    assert!(
        catalogue::materialize_rows(
            &source.inventory,
            &source.registry,
            &rows,
            &physical,
            Shape::LibraryTopDeckPair
        )
        .is_err(),
        "unused Consider must still authenticate before Island-only subset"
    );
    assert!(
        source
            .opening(
                [TeachingDeck {
                    opt: 5,
                    ..TeachingDeck::default()
                }; 2],
                EntropyTape::new(vec![])
            )
            .is_err()
    );
    let mut incomplete = physical.clone();
    incomplete.pop();
    assert!(
        catalogue::materialize_rows(
            &source.inventory,
            &source.registry,
            &source.definitions,
            &incomplete,
            Shape::LibraryTopDeckPair
        )
        .is_err()
    );
    let mut duplicate = physical.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(
        catalogue::materialize_rows(
            &source.inventory,
            &source.registry,
            &source.definitions,
            &duplicate,
            Shape::LibraryTopDeckPair
        )
        .is_err()
    );
    let mut foreign = physical.clone();
    foreign[0].owner = spellwake_engine::priority::SeatId(999);
    assert!(
        catalogue::materialize_rows(
            &source.inventory,
            &source.registry,
            &source.definitions,
            &foreign,
            Shape::LibraryTopDeckPair
        )
        .is_err()
    );
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn source4_rejects_mutated_bytes_and_foreign_native_vocabulary() {
    let source = genuine();
    let metadata = std::fs::read(input("SPELLWAKE_CATALOGUE_METADATA")).unwrap();
    let snapshot = std::fs::read(input("SPELLWAKE_CATALOGUE_INPUT")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let meta = dir.path().join("metadata");
    let gzip = dir.path().join("snapshot");
    std::fs::write(&meta, &metadata).unwrap();
    std::fs::write(&gzip, &snapshot).unwrap();
    let mut changed = metadata.clone();
    changed[0] ^= 1;
    std::fs::write(&meta, &changed).unwrap();
    assert_eq!(
        LibraryTopSource::load(&meta, &gzip, source.vocabulary.clone())
            .err()
            .unwrap()
            .to_string(),
        "library-top metadata bytes mismatch"
    );
    std::fs::write(&meta, &metadata).unwrap();
    let mut changed = snapshot.clone();
    changed[0] ^= 1;
    std::fs::write(&gzip, &changed).unwrap();
    assert_eq!(
        LibraryTopSource::load(&meta, &gzip, source.vocabulary.clone())
            .err()
            .unwrap()
            .to_string(),
        "library-top snapshot bytes mismatch"
    );
    std::fs::write(&gzip, &snapshot).unwrap();
    let foreign = Arc::new(
        CreatureTypeVocabulary::new("negative-only".into(), [0; 32], vec!["Cat".into()]).unwrap(),
    );
    assert_eq!(
        LibraryTopSource::load(&meta, &gzip, foreign)
            .err()
            .unwrap()
            .to_string(),
        "library-top native vocabulary mismatch"
    );
}

#[test]
fn unavailable_source_preserves_original_io_error_without_fallback() {
    use std::error::Error;
    let foreign = Arc::new(
        CreatureTypeVocabulary::new("negative-io-only".into(), [0; 32], vec!["Cat".into()])
            .unwrap(),
    );
    let dir = tempfile::tempdir().unwrap();
    let error = LibraryTopSource::load(
        &dir.path().join("missing"),
        &dir.path().join("missing-gzip"),
        foreign,
    )
    .err()
    .unwrap();
    assert!(matches!(
        &error,
        LibraryTopSourceError::Io {
            stage: "metadata",
            ..
        }
    ));
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .is_some()
    );
}
#[test]
#[ignore = "requires explicit genuine full metadata/gzip/native vocabulary inputs"]
fn unused_native_source_printing_failure_stays_typed_before_physical_subset() {
    use std::error::Error;
    let mut source = LibraryTopSource::load(
        &input("SPELLWAKE_CATALOGUE_METADATA"),
        &input("SPELLWAKE_CATALOGUE_INPUT"),
        genuine().vocabulary.clone(),
    )
    .unwrap();
    let unused = source
        .inventory
        .cards
        .iter_mut()
        .find(|row| row.oracle_id.as_deref() == Some(CONSIDER))
        .unwrap();
    unused.scryfall_id = "00000000-0000-0000-0000-000000000000".into();
    let error = source
        .opening([TeachingDeck::default(); 2], EntropyTape::new(vec![]))
        .err()
        .unwrap();
    assert!(matches!(
        &error,
        LibraryTopSourceError::Registry(RegistryError::LibraryTop(
            spellwake_card_programs::LibraryTopRegistryError::PrintingMismatch {
                oracle_id: CONSIDER
            }
        ))
    ));
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<RegistryError>()
            .is_some()
    );
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<spellwake_card_programs::LibraryTopRegistryError>()
            .is_some()
    );
}
