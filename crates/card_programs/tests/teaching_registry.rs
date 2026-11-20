//! Finite teaching admission and source-guard regressions. The embedded subset
//! is explicitly synthetic and cannot qualify the full pinned catalogue intake.
use serde_json::Value;
use spellwake_card_programs::{
    BindingStatus, LION, OracleId, ProgramKey, Registry, RegistryError, Scope, TEACHING_PROFILE,
    TEACHING_REGISTRY_VERSION, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};
use spellwake_content::{Inventory, Legality, snapshot::decode};
use spellwake_contracts::{
    RulesInteger,
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyId},
};
use spellwake_engine::{
    characteristics::{Color, PowerToughness},
    land::LandType,
    mana::{ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{CardType, ProgramKind, Supertype},
};
use std::collections::BTreeSet;

fn vocabulary() -> CreatureTypeVocabulary {
    let path = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("teaching tests require the explicit retained source-qualified vocabulary");
    let raw: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let hash =
        CreatureVocabularyId::try_from(raw["rules_sha256"].as_str().unwrap().to_owned()).unwrap();
    let members = raw["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member.as_str().unwrap().to_owned())
        .collect();
    let value = CreatureTypeVocabulary::new(
        raw["rules_revision"].as_str().unwrap().to_owned(),
        *hash.as_bytes(),
        members,
    )
    .unwrap();
    assert_eq!(value.id().to_string(), raw["id"].as_str().unwrap());
    assert_eq!(value.id().to_string(), TEACHING_VOCABULARY_ID);
    value
}
fn subset() -> Inventory {
    decode(
        include_bytes!("../../../tests/catalogue/teaching-cards.meta.json"),
        include_bytes!("../../../tests/catalogue/teaching-cards.jsonl.gz").to_vec(),
    )
    .unwrap()
}
/// Model the post-intake source identity solely for guard unit tests. The actual
/// compressed subset has another digest and must fail unmodified admission.
fn guard_inventory() -> Inventory {
    let mut source = subset();
    source.compressed_sha256 = TEACHING_SOURCE_SHA256.into();
    source
}
fn id(value: &str) -> OracleId {
    value.to_owned().try_into().unwrap()
}
fn expected() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../tests/catalogue/teaching-cards.expected.json"
    ))
    .unwrap()
}

#[test]
fn teaching_report_contains_exactly_selected_twenty_and_preserves_global_qualification_boundary() {
    let source = guard_inventory();
    let registry = Registry::from_teaching_inventory(&source, &vocabulary()).unwrap();
    let coverage = registry.coverage();
    assert_eq!(coverage.registry_version, TEACHING_REGISTRY_VERSION);
    assert_eq!(coverage.source_sha256, TEACHING_SOURCE_SHA256);
    assert_eq!(
        coverage.creature_vocabulary_id.to_string(),
        TEACHING_VOCABULARY_ID
    );
    assert_eq!(coverage.source_records, 20);
    assert_eq!(coverage.required_records, 20);
    assert_eq!(coverage.required_partial_bindings, 20);
    assert_eq!(coverage.required_without_binding, 0);
    assert_eq!(coverage.fully_qualified_cards, 0);
    assert_eq!(coverage.behavior_evidence, "not-evaluated-by-registry");
    assert_eq!(coverage.cards.len(), 20);
    assert_eq!(TEACHING_PROFILE, "teaching-vanilla-v1");
    for expected in expected()["cards"].as_array().unwrap() {
        let card = coverage
            .cards
            .iter()
            .find(|card| card.oracle_id.as_deref() == expected["oracleId"].as_str())
            .unwrap();
        let BindingStatus::Partial {
            program,
            scope,
            semantic_sha256,
        } = &card.binding
        else {
            panic!("selected card must retain conservative partial status");
        };
        assert_eq!(*scope, Scope::TeachingOrdinaryCardV1);
        assert_eq!(
            semantic_sha256,
            expected["registrySignatureV1Sha256"].as_str().unwrap()
        );
        assert_eq!(
            *program,
            if matches!(card.name.as_str(), "Plains" | "Forest") {
                ProgramKey::TeachingBasicLandV1
            } else {
                ProgramKey::TeachingVanillaCreatureV1
            }
        );
    }
    assert_eq!(
        serde_json::to_value(ProgramKey::TeachingBasicLandV1).unwrap(),
        "teaching-basic-land-v1"
    );
    assert_eq!(
        serde_json::to_value(ProgramKey::TeachingVanillaCreatureV1).unwrap(),
        "teaching-vanilla-creature-v1"
    );
    assert_eq!(
        serde_json::to_value(Scope::TeachingOrdinaryCardV1).unwrap(),
        "teaching-ordinary-card-v1"
    );
}

#[test]
fn all_twenty_library_seeds_preserve_source_costs_stats_types_subtypes_names_and_basic_mana_origin()
{
    let source = guard_inventory();
    let vocabulary = vocabulary();
    let registry = Registry::from_teaching_inventory(&source, &vocabulary).unwrap();
    for (ordinal, card) in source.cards.iter().enumerate() {
        let oracle = id(card.oracle_id.as_deref().unwrap());
        let card_id = CardId(u64::try_from(ordinal).unwrap());
        let seed = registry
            .seed(
                &oracle,
                card_id,
                SeatId(1),
                ObjectDestination::LibraryBottom,
            )
            .unwrap();
        assert_eq!(seed.card, card_id);
        assert_eq!(seed.owner, SeatId(1));
        assert_eq!(seed.destination, ObjectDestination::LibraryBottom);
        let program = seed.program;
        program.validate(&vocabulary).unwrap();
        assert_eq!(
            program.names,
            spellwake_contracts::names::ObjectNames::single(
                spellwake_contracts::names::EnglishCardName::new(card.name.clone()).unwrap()
            )
        );
        assert!(
            program.mana.is_empty(),
            "basic-land abilities must derive intrinsically once"
        );
        assert!(program.activated.is_empty());
        assert!(program.triggered.is_empty());
        assert!(program.continuous.is_none());
        let ProgramKind::Permanent(permanent) = &program.kind else {
            panic!("ordinary permanent required")
        };
        let face = &card.faces[0];
        if let Some(power) = &face.power {
            assert_eq!(program.card_types, BTreeSet::from([CardType::Creature]));
            assert!(program.supertypes.is_empty());
            assert!(program.land_subtypes.is_empty());
            let (type_family, source_subtypes) = face
                .type_line
                .as_deref()
                .unwrap()
                .split_once(" — ")
                .unwrap();
            assert_eq!(type_family, "Creature");
            assert_eq!(
                program.creature_subtypes,
                source_subtypes
                    .split(' ')
                    .map(|label| vocabulary.resolve_label(label).unwrap())
                    .collect()
            );
            assert_eq!(
                permanent.printed_pt,
                Some(
                    PowerToughness {
                        power: RulesInteger::try_from(power.clone()).unwrap(),
                        toughness: RulesInteger::try_from(face.toughness.clone().unwrap()).unwrap(),
                    }
                    .into()
                )
            );
            let mut cost = ManaCost::free();
            for part in face
                .mana_cost
                .as_deref()
                .unwrap()
                .trim_matches(['{', '}'])
                .split("}{")
            {
                match part {
                    "W" | "G" => {
                        let mana_type = if part == "W" {
                            ManaType::White
                        } else {
                            ManaType::Green
                        };
                        let quantity = cost.required.entry(mana_type).or_insert_with(|| 0.into());
                        *quantity = quantity.plus(&1.into());
                    }
                    generic => cost.generic = RulesInteger::try_from(generic.to_owned()).unwrap(),
                }
            }
            assert_eq!(permanent.casting_cost, Some(cost));
            assert_eq!(
                program.defined_colors(),
                BTreeSet::from([if face.mana_cost.as_deref().unwrap().contains('W') {
                    Color::White
                } else {
                    Color::Green
                }])
            );
        } else {
            assert_eq!(program.card_types, BTreeSet::from([CardType::Land]));
            assert_eq!(program.supertypes, BTreeSet::from([Supertype::Basic]));
            assert_eq!(
                program.land_subtypes,
                BTreeSet::from([if card.name == "Plains" {
                    LandType::Plains
                } else {
                    LandType::Forest
                }])
            );
            assert!(program.creature_subtypes.is_empty());
            assert!(permanent.casting_cost.is_none());
            assert!(permanent.printed_pt.is_none());
            assert!(program.defined_colors().is_empty());
        }
    }
}

#[test]
fn unmodified_subset_and_foreign_full_source_identity_are_rejected() {
    let mut source = subset();
    assert_eq!(
        source.compressed_sha256,
        expected()["subsetSha256"].as_str().unwrap()
    );
    assert!(matches!(
        Registry::from_teaching_inventory(&source, &vocabulary()),
        Err(RegistryError::TeachingSourceMismatch)
    ));
    source.compressed_sha256 = "0".repeat(64);
    assert!(matches!(
        Registry::from_teaching_inventory(&source, &vocabulary()),
        Err(RegistryError::TeachingSourceMismatch)
    ));
}

#[test]
fn foreign_vocabulary_is_rejected_even_with_all_selected_member_labels_present() {
    let vocabulary = vocabulary();
    let foreign = CreatureTypeVocabulary::new(
        "synthetic-foreign-rules".into(),
        *vocabulary.rules_sha256(),
        vocabulary.members().to_vec(),
    )
    .unwrap();
    assert!(matches!(
        Registry::from_teaching_inventory(&guard_inventory(), &foreign),
        Err(RegistryError::TeachingVocabularyMismatch)
    ));
}

#[test]
fn every_missing_or_duplicate_selected_identity_rejects_whole_registry_admission() {
    let vocabulary = vocabulary();
    for index in 0..20 {
        let mut missing = guard_inventory();
        missing.cards.remove(index);
        assert!(matches!(
            Registry::from_teaching_inventory(&missing, &vocabulary),
            Err(RegistryError::TeachingMissingIdentity { .. })
        ));
        let mut duplicate = guard_inventory();
        let extra = subset().cards.remove(index);
        duplicate.cards.push(extra);
        assert!(matches!(
            Registry::from_teaching_inventory(&duplicate, &vocabulary),
            Err(RegistryError::TeachingDuplicateIdentity { .. })
        ));
        let mut foreign = guard_inventory();
        foreign.cards[index].oracle_id = Some("00000000-0000-0000-0000-000000000001".into());
        assert!(matches!(
            Registry::from_teaching_inventory(&foreign, &vocabulary),
            Err(RegistryError::TeachingMissingIdentity { .. })
        ));
    }
}

#[test]
fn every_selected_face_field_is_guarded_including_missing_empty_text_and_extra_faces() {
    let vocabulary = vocabulary();
    for index in 0..20 {
        for field in [
            "name",
            "layout",
            "face-name",
            "type",
            "cost",
            "text",
            "missing-text",
            "power",
            "toughness",
            "missing-face",
            "extra-face",
        ] {
            let mut source = guard_inventory();
            let extra = subset().cards.remove(index).faces.remove(0);
            let card = &mut source.cards[index];
            match field {
                "name" => card.name.push('!'),
                "layout" => card.layout = "transform".into(),
                "face-name" => card.faces[0].name.push('!'),
                "type" => card.faces[0].type_line = Some("Artifact".into()),
                "cost" => card.faces[0].mana_cost = None,
                "text" => card.faces[0].oracle_text = Some("Draw a card.".into()),
                "missing-text" => card.faces[0].oracle_text = None,
                "power" => card.faces[0].power = Some("123".into()),
                "toughness" => card.faces[0].toughness = Some("456".into()),
                "missing-face" => card.faces.clear(),
                "extra-face" => card.faces.push(extra),
                _ => unreachable!(),
            }
            assert!(
                matches!(
                    Registry::from_teaching_inventory(&source, &vocabulary),
                    Err(RegistryError::TeachingSemanticMismatch { .. })
                ),
                "index={index} field={field}"
            );
        }
    }
}

#[test]
fn every_selected_modern_legality_and_required_flag_are_guarded() {
    let vocabulary = vocabulary();
    for index in 0..20 {
        for legality in [
            Some(Legality::Banned),
            Some(Legality::Restricted),
            Some(Legality::NotLegal),
            Some(Legality::Unknown("future".into())),
            None,
        ] {
            let mut source = guard_inventory();
            if let Some(legality) = legality {
                source.cards[index]
                    .legalities
                    .insert("modern".into(), legality);
            } else {
                source.cards[index].legalities.remove("modern");
            }
            assert!(matches!(
                Registry::from_teaching_inventory(&source, &vocabulary),
                Err(RegistryError::TeachingLegalityMismatch { .. })
            ));
        }
        let mut source = guard_inventory();
        source.cards[index].required_by_base_formats = false;
        assert!(matches!(
            Registry::from_teaching_inventory(&source, &vocabulary),
            Err(RegistryError::TeachingLegalityMismatch { .. })
        ));
    }
}

#[test]
fn unrelated_source_cards_are_not_admitted_or_reported_as_teaching_bindings() {
    let mut source = guard_inventory();
    let mut extra = subset().cards.remove(0);
    let unknown = "00000000-0000-0000-0000-000000000001";
    extra.oracle_id = Some(unknown.into());
    source.cards.push(extra);
    let registry = Registry::from_teaching_inventory(&source, &vocabulary()).unwrap();
    assert_eq!(registry.coverage().source_records, 21);
    assert_eq!(registry.coverage().required_records, 20);
    assert_eq!(registry.coverage().cards.len(), 20);
    assert!(registry.resolve(&id(unknown)).is_none());
    assert!(matches!(
        registry.seed(
            &id(unknown),
            CardId(100),
            SeatId(0),
            ObjectDestination::LibraryBottom
        ),
        Err(RegistryError::MissingBinding)
    ));
}

#[test]
fn default_reference_registry_retains_its_version_and_battlefield_only_lion_scope() {
    let source = guard_inventory();
    let registry = Registry::from_inventory(&source, &vocabulary()).unwrap();
    assert_eq!(registry.coverage().registry_version, 4);
    assert_eq!(
        registry.resolve(&id(LION)).unwrap().key(),
        ProgramKey::LionBattlefieldV2
    );
    assert_eq!(
        registry.resolve(&id(LION)).unwrap().scope(),
        Scope::SeededVanillaBattlefieldOnly
    );
    assert!(matches!(
        registry.seed(
            &id(LION),
            CardId(1),
            SeatId(0),
            ObjectDestination::LibraryBottom
        ),
        Err(RegistryError::UnsupportedPlacement)
    ));
    let seeded = registry
        .seed(
            &id(LION),
            CardId(1),
            SeatId(0),
            ObjectDestination::Battlefield {
                controller: SeatId(0),
            },
        )
        .unwrap();
    let ProgramKind::Permanent(permanent) = seeded.program.kind else {
        panic!("expected creature")
    };
    assert!(permanent.casting_cost.is_none());
}
