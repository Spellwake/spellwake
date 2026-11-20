//! Prepared-input adapter tests. Source-dependent tests are explicitly opt-in.
use super::*;
use crate::ordinary_game::{
    catalogue,
    fixture::{A, B},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_card_programs::{BindingStatus, RegistryError};
use spellwake_engine::{
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::{Arc, OnceLock},
};

fn input() -> Value {
    json!({"kind":"selection_prepared","turn":3,"step":"precombat-main","active":"P0",
        "life":{"P0":"20","P1":"20"},
        "zones":[{"role":"P0","hand":[],"library_top_first":[],"graveyard_top_first":[],"exile":[]},
                 {"role":"P1","hand":[],"library_top_first":[],"graveyard_top_first":[],"exile":[]}],
        "battlefield_oldest_first":[],"floating_mana":[]})
}
fn decode(value: Value) -> SelectionInitialization {
    serde_json::from_value(value).expect("explicit prepared adapter input")
}

#[test]
fn strict_new_shape_requires_fields_and_rejects_old_kind_and_unknown_members() {
    let valid = input();
    let object = valid.as_object().unwrap();
    for field in object.keys() {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<SelectionInitialization>(missing).is_err(),
            "missing {field}"
        );
    }
    let mut unknown = valid.clone();
    unknown["extra"] = json!(null);
    assert!(serde_json::from_value::<SelectionInitialization>(unknown).is_err());
    let mut old = valid.clone();
    old["kind"] = json!("scheduled");
    assert!(serde_json::from_value::<SelectionInitialization>(old).is_err());
    assert!(
        serde_json::from_value::<crate::ordinary_game::mandatory::data::Initialization>(valid)
            .is_err()
    );
    let duplicate = serde_json::to_string(&input())
        .unwrap()
        .replacen("{", "{\"turn\":4,", 1);
    assert!(serde_json::from_str::<SelectionInitialization>(&duplicate).is_err());
}

#[test]
fn new_mana_uses_six_exact_labels_and_existing_canonical_integer_contract() {
    for (index, name) in ["White", "Blue", "Black", "Red", "Green", "Colourless"]
        .into_iter()
        .enumerate()
    {
        let mut raw = input();
        raw["floating_mana"] = json!([{"player":"P0","mana_type":name,"amount":"0"}]);
        let SelectionInitialization::SelectionPrepared { floating_mana, .. } = decode(raw);
        assert_eq!(floating_mana[0].mana_type.native(), ManaType::ALL[index]);
        assert_eq!(floating_mana[0].amount, 0.into());
    }
    for amount in [
        json!(1),
        json!(null),
        json!("+1"),
        json!("01"),
        json!("-0"),
        json!("1.0"),
    ] {
        let mut raw = input();
        raw["floating_mana"] = json!([{"player":"P0","mana_type":"Black","amount":amount}]);
        assert!(serde_json::from_value::<SelectionInitialization>(raw).is_err());
    }
    let mut raw = input();
    raw["floating_mana"] =
        json!([{"player":"P0","mana_type":"Colourless","amount":"184467440737095516160000000000"}]);
    let SelectionInitialization::SelectionPrepared { floating_mana, .. } = decode(raw);
    assert_eq!(
        String::from(floating_mana[0].amount.clone()),
        "184467440737095516160000000000"
    );
    assert!(
        serde_json::from_value::<crate::ordinary_game::mandatory::data::Mana>(json!("Colourless"))
            .is_err(),
        "old five-type grammar stays exact"
    );
}

#[test]
fn strict_nested_roles_mana_and_resource_fields_reject_wrong_shapes() {
    for raw in [
        {
            let mut raw = input();
            raw["floating_mana"] = json!([{"player":"P2","mana_type":"Black","amount":"1"}]);
            raw
        },
        {
            let mut raw = input();
            raw["floating_mana"] = json!([{"player":"P0","mana_type":"Colorless","amount":"1"}]);
            raw
        },
        {
            let mut raw = input();
            raw["floating_mana"] =
                json!([{"player":"P0","mana_type":"Black","amount":"1","generated":true}]);
            raw
        },
        {
            let mut raw = input();
            raw["floating_mana"] = json!([{"player":"P0","mana_type":"Black"}]);
            raw
        },
        {
            let mut raw = input();
            raw["zones"][0]["invented"] = json!([]);
            raw
        },
        {
            let mut raw = input();
            raw["life"]["P2"] = json!("20");
            raw
        },
        {
            let mut raw = input();
            raw["turn"] = json!(-1);
            raw
        },
        {
            let mut raw = input();
            raw["step"] = json!("draw");
            raw
        },
    ] {
        assert!(serde_json::from_value::<SelectionInitialization>(raw).is_err());
    }
}

struct Source {
    inventory: Inventory,
    registry: Registry,
    vocabulary: Arc<CreatureTypeVocabulary>,
    sources: Vec<DefinitionInput>,
}
fn path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit {name} required; no synthetic source fallback"))
}
fn source() -> &'static Source {
    static SOURCE: OnceLock<Source> = OnceLock::new();
    SOURCE.get_or_init(|| {
        let metadata = path("SPELLWAKE_CATALOGUE_METADATA");
        let snapshot = path("SPELLWAKE_CATALOGUE_INPUT");
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(&metadata).unwrap())),
            "77a7ffc2250306197da78c0c4faf35fb4fae01b4dbdd50bf8eb45f502e4737ad"
        );
        let vocabulary =
            crate::creature_types::selected_vocabulary().expect("explicit genuine vocabulary");
        let inventory =
            spellwake_content::snapshot::load(&metadata, &snapshot).expect("actual full catalogue");
        let registry = Registry::from_resolution_selection_inventory(&inventory, &vocabulary)
            .expect("actual guarded source9 Registry");
        assert_eq!(inventory.source_records, 39196);
        assert_eq!(registry.coverage().registry_version, 9);
        assert_eq!(registry.coverage().cards.len(), 9);
        assert_eq!(registry.coverage().fully_qualified_cards, 0);
        let wanted = registry
            .coverage()
            .cards
            .iter()
            .map(|c| c.oracle_id.as_ref().unwrap().as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut raw_sources = BTreeMap::new();
        let mut reader = BufReader::new(flate2::read::MultiGzDecoder::new(
            std::fs::File::open(snapshot).unwrap(),
        ));
        let mut raw = vec![];
        let mut line = 0u64;
        while reader.read_until(b'\n', &mut raw).unwrap() != 0 {
            line += 1;
            let value: Value = serde_json::from_slice(&raw).unwrap();
            if let Some(oracle) = value["oracle_id"].as_str().filter(|id| wanted.contains(id)) {
                assert!(
                    raw_sources
                        .insert(
                            oracle.to_owned(),
                            (line, format!("{:x}", Sha256::digest(&raw)), value)
                        )
                        .is_none()
                );
            }
            raw.clear();
        }
        assert_eq!(raw_sources.len(), 9);
        let sources = registry
            .coverage()
            .cards
            .iter()
            .map(|coverage| {
                let id = coverage.oracle_id.as_ref().unwrap();
                let (line, hash, raw) = &raw_sources[id];
                let BindingStatus::Partial {
                    semantic_sha256, ..
                } = &coverage.binding
                else {
                    panic!("source9 must remain Partial")
                };
                DefinitionInput {
                    name: raw["name"].as_str().unwrap().to_owned(),
                    oracle_id: id.clone(),
                    scryfall_id: raw["id"].as_str().unwrap().to_owned(),
                    source_line_one_based: *line,
                    raw_line_sha256_including_lf: hash.clone(),
                    registry_signature_v1_sha256: semantic_sha256.clone(),
                    set: raw["set"].as_str().unwrap().to_owned(),
                    collector: raw["collector_number"].as_str().unwrap().to_owned(),
                    language: raw["lang"].as_str().unwrap().to_owned(),
                }
            })
            .collect();
        Source {
            inventory,
            registry,
            vocabulary,
            sources,
        }
    })
}
fn rows(source: &Source) -> Vec<PhysicalRow> {
    source
        .sources
        .iter()
        .enumerate()
        .map(|(i, definition)| PhysicalRow {
            alias: format!("source.{}", definition.name),
            card: CardId(u64::try_from(i).unwrap() + 1),
            owner: if definition.name == "Grizzly Bears" {
                B
            } else {
                A
            },
            oracle_id: definition.oracle_id.clone(),
            copy: 1,
        })
        .collect()
}
fn prepared(rows: &[PhysicalRow]) -> Value {
    let mut raw = input();
    let owned = rows
        .iter()
        .filter(|row| row.owner == A)
        .map(|row| row.alias.clone())
        .collect::<Vec<_>>();
    assert_eq!(owned.len(), 8);
    raw["zones"][0]["hand"] = json!(&owned[..2]);
    raw["zones"][0]["library_top_first"] = json!(&owned[2..4]);
    raw["zones"][0]["graveyard_top_first"] = json!(&owned[4..6]);
    raw["zones"][0]["exile"] = json!(&owned[6..]);
    raw["battlefield_oldest_first"] = json!([{"alias":rows.iter().find(|row|row.owner==B).unwrap().alias,"controller":"P0","tapped":false}]);
    raw["floating_mana"] = json!([{"player":"P0","mana_type":"Black","amount":"0"},{"player":"P1","mana_type":"Colourless","amount":"184467440737095516160000000000"}]);
    raw
}
fn lower(
    source: &Source,
    rows: &[PhysicalRow],
    raw: Value,
) -> Result<PreparedMaterialized, String> {
    materialize_prepared(
        &source.inventory,
        &source.registry,
        &source.vocabulary,
        &source.sources,
        rows,
        &decode(raw),
    )
}

#[test]
#[ignore = "requires explicit genuine full catalogue metadata/input and creature vocabulary; no fallback"]
fn genuine_source9_preparation_preserves_source_programs_orders_and_seeded_resources() {
    let source = source();
    let rows = rows(source);
    let raw = prepared(&rows);
    let output = lower(source, &rows, raw.clone()).expect("actual source9 prepared lowering");
    assert_eq!(
        output.start,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain
        }
    );
    assert_eq!(output.active, A);
    assert_eq!(output.materialized.definitions.len(), 9);
    assert_eq!(output.materialized.aliases.len(), 9);
    assert_eq!(
        output
            .life
            .iter()
            .map(|row| (row.player, row.total.clone()))
            .collect::<Vec<_>>(),
        vec![(A, 20.into()), (B, 20.into())]
    );
    assert_eq!(output.mana[0].amount, 0.into());
    assert_eq!(output.mana[1].mana_type, ManaType::Colourless);
    assert_eq!(
        String::from(output.mana[1].amount.clone()),
        "184467440737095516160000000000"
    );
    let by_alias = rows
        .iter()
        .map(|row| (row.alias.as_str(), row.card))
        .collect::<BTreeMap<_, _>>();
    let expected = raw["zones"][0]["hand"]
        .as_array()
        .unwrap()
        .iter()
        .chain(raw["zones"][0]["library_top_first"].as_array().unwrap())
        .chain(
            raw["zones"][0]["graveyard_top_first"]
                .as_array()
                .unwrap()
                .iter()
                .rev(),
        )
        .chain(raw["zones"][0]["exile"].as_array().unwrap())
        .map(|alias| by_alias[alias.as_str().unwrap()])
        .chain(output.initial_battlefield_order.iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(
        output
            .materialized
            .cards
            .iter()
            .map(|card| card.card)
            .collect::<Vec<_>>(),
        expected
    );
    let battlefield = rows.iter().find(|row| row.owner == B).unwrap();
    assert_eq!(output.initial_battlefield_order, vec![battlefield.card]);
    for seed in &output.materialized.cards {
        let physical = rows.iter().find(|row| row.card == seed.card).unwrap();
        let id = spellwake_card_programs::OracleId::try_from(physical.oracle_id.clone()).unwrap();
        assert_eq!(
            seed.program,
            source
                .registry
                .seed(&id, seed.card, seed.owner, ObjectDestination::LibraryBottom)
                .unwrap()
                .program
        );
        if seed.card == battlefield.card {
            assert_eq!(seed.owner, B);
            assert_eq!(
                seed.destination,
                ObjectDestination::Battlefield { controller: A }
            );
        }
    }
    // A reduced physical subset still requires the complete genuine9 definition intake.
    let only = vec![battlefield.clone()];
    let mut reduced = input();
    reduced["zones"][1]["hand"] = json!([battlefield.alias]);
    let output = lower(source, &only, reduced).unwrap();
    assert_eq!(output.materialized.cards.len(), 1);
    assert!(
        materialize_prepared(
            &source.inventory,
            &source.registry,
            &source.vocabulary,
            &source.sources[..8],
            &only,
            &decode(input())
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires explicit genuine full catalogue metadata/input and creature vocabulary; no fallback"]
fn source9_materializer_rejects_source_swaps_and_preserves_closed_old_shapes() {
    let source = source();
    let rows = rows(source);
    let init = decode(prepared(&rows));
    for mutate in [0, 1, 2, 3] {
        let mut definitions = source.sources.clone();
        match mutate {
            0 => definitions[0].oracle_id = definitions[1].oracle_id.clone(),
            1 => definitions[0].scryfall_id = definitions[1].scryfall_id.clone(),
            2 => {
                definitions[0].registry_signature_v1_sha256 =
                    definitions[1].registry_signature_v1_sha256.clone()
            }
            _ => definitions[0].registry_signature_v1_sha256 = "0".repeat(64),
        }
        assert!(
            materialize_prepared(
                &source.inventory,
                &source.registry,
                &source.vocabulary,
                &definitions,
                &rows,
                &init
            )
            .is_err()
        );
    }
    for shape in [
        Shape::HistoricalDeckPair,
        Shape::MandatoryDeckPair,
        Shape::MandatoryPrepared,
        Shape::OptionalPaymentDeckPair,
        Shape::OptionalPaymentPrepared,
    ] {
        assert!(
            catalogue::materialize_rows(
                &source.inventory,
                &source.registry,
                &source.sources,
                &rows,
                shape
            )
            .is_err()
        );
    }
    let mut duplicate = rows.clone();
    duplicate[1].card = duplicate[0].card;
    assert!(
        materialize_prepared(
            &source.inventory,
            &source.registry,
            &source.vocabulary,
            &source.sources,
            &duplicate,
            &init
        )
        .is_err()
    );
    let mut foreign = rows.clone();
    foreign[0].owner = SeatId(99);
    assert!(
        materialize_prepared(
            &source.inventory,
            &source.registry,
            &source.vocabulary,
            &source.sources,
            &foreign,
            &init
        )
        .is_err()
    );
}

#[test]
#[ignore = "requires explicit genuine full catalogue metadata/input and creature vocabulary; no fallback"]
fn genuine_prepared_lowering_rejects_negative_duplicate_and_untrue_partition_premises() {
    let source = source();
    let rows = rows(source);
    let raw = prepared(&rows);
    let mut corruptions = vec![];
    {
        let mut bad = raw.clone();
        bad["floating_mana"][0]["amount"] = json!("-1");
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["floating_mana"]
            .as_array_mut()
            .unwrap()
            .push(raw["floating_mana"][0].clone());
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["battlefield_oldest_first"][0]["tapped"] = json!(true);
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["zones"][0]["hand"][0] = json!("unknown");
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["zones"][0]["hand"][0] = raw["zones"][0]["hand"][1].clone();
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["zones"][0]["exile"] = json!([]);
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["zones"].as_array_mut().unwrap().reverse();
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["zones"][0]["hand"]
            .as_array_mut()
            .unwrap()
            .push(raw["battlefield_oldest_first"][0]["alias"].clone());
        bad["battlefield_oldest_first"] = json!([]);
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["turn"] = json!(0);
        corruptions.push(bad);
    }
    {
        let mut bad = raw.clone();
        bad["life"]["P0"] = json!("020");
        corruptions.push(bad);
    }
    for bad in corruptions {
        assert!(lower(source, &rows, bad).is_err());
    }
}

#[test]
#[ignore = "requires explicit genuine full catalogue metadata/input and creature vocabulary; SP01 provenance refusal"]
fn genuine_source9_preparation_rejects_separately_changed_inventory_hash() {
    let source = source();
    let rows = rows(source);
    let mut inventory = spellwake_content::snapshot::load(
        &path("SPELLWAKE_CATALOGUE_METADATA"),
        &path("SPELLWAKE_CATALOGUE_INPUT"),
    )
    .unwrap();
    inventory.compressed_sha256 = "0".repeat(64);
    let result = materialize_prepared(
        &inventory,
        &source.registry,
        &source.vocabulary,
        &source.sources,
        &rows,
        &decode(prepared(&rows)),
    );
    let emitted = result
        .as_ref()
        .ok()
        .and_then(|output| output.materialized.definitions.first())
        .map(|definition| definition.sources[0].sha256.clone());
    assert!(
        result.is_err(),
        "SP01: separate changed Inventory was accepted; emitted source {emitted:?}; guarded Registry source {}",
        source.registry.coverage().source_sha256
    );
}

#[test]
#[ignore = "requires explicit genuine full catalogue metadata/input and creature vocabulary; source faults are refusal controls"]
fn genuine_source9_owner_rejects_separate_metadata_face_vocabulary_and_other_profile() {
    let source = source();
    let rows = rows(source);
    let initialization = decode(prepared(&rows));
    let mut inventory = spellwake_content::snapshot::load(
        &path("SPELLWAKE_CATALOGUE_METADATA"),
        &path("SPELLWAKE_CATALOGUE_INPUT"),
    )
    .unwrap();
    let refuse_leaf = |inventory: &Inventory,
                       registry: &Registry,
                       vocabulary: &CreatureTypeVocabulary,
                       error: &RegistryError| {
        assert_eq!(
            materialize_prepared(
                inventory,
                registry,
                vocabulary,
                &source.sources,
                &rows,
                &initialization,
            )
            .err()
            .expect("source owner must reject before physical lowering"),
            format!("selection preparation source validation: {error}")
        );
    };
    let refuse_source = |inventory: &Inventory, mutation: &str| {
        let error = source
            .registry
            .validate_resolution_selection_source(inventory, &source.vocabulary)
            .unwrap_err();
        assert!(matches!(
            &error,
            RegistryError::ResolutionSelectionSourceMismatch
        ));
        refuse_leaf(inventory, &source.registry, &source.vocabulary, &error);
        println!("SP01 refusal {mutation}: {error:?}");
    };
    let hash = std::mem::replace(&mut inventory.compressed_sha256, "0".repeat(64));
    refuse_source(&inventory, "separate-inventory-sha256");
    inventory.compressed_sha256 = hash;
    let uri = std::mem::replace(
        &mut inventory.source.jsonl_download_uri,
        "https://invalid.example/SP01-foreign-source".into(),
    );
    refuse_source(&inventory, "separate-inventory-uri");
    inventory.source.jsonl_download_uri = uri;
    let updated = std::mem::replace(&mut inventory.source.updated_at, "SP01-foreign-date".into());
    refuse_source(&inventory, "separate-inventory-updated-at");
    inventory.source.updated_at = updated;
    let oracle = &source.sources[0].oracle_id;
    let index = inventory
        .cards
        .iter()
        .position(|card| card.oracle_id.as_ref() == Some(oracle))
        .unwrap();
    let text = inventory.cards[index].faces[0]
        .oracle_text
        .replace("SP01 altered complete selected face".into());
    let error = source
        .registry
        .validate_resolution_selection_source(&inventory, &source.vocabulary)
        .unwrap_err();
    match &error {
        RegistryError::ResolutionSelectionSemanticMismatch { oracle_id } => {
            assert_eq!(*oracle_id, oracle.as_str());
        }
        other => panic!("wrong actual source guard: {other:?}"),
    }
    refuse_leaf(&inventory, &source.registry, &source.vocabulary, &error);
    println!("SP01 refusal separate-selected-face: {error:?}");
    inventory.cards[index].faces[0].oracle_text = text;

    // Synthetic fault input only; never a replacement for genuine qualification V.
    let foreign = CreatureTypeVocabulary::new(
        "SYNTHETIC-SP01-FOREIGN".into(),
        [0; 32],
        vec!["Zombie".into()],
    )
    .unwrap();
    let error = source
        .registry
        .validate_resolution_selection_source(&inventory, &foreign)
        .unwrap_err();
    assert!(matches!(
        &error,
        RegistryError::ResolutionSelectionVocabularyMismatch
    ));
    refuse_leaf(&inventory, &source.registry, &foreign, &error);
    println!("SP01 refusal foreign-vocabulary: {error:?}");

    let natural =
        Registry::from_resolution_selection_natural_mana_inventory(&inventory, &source.vocabulary)
            .unwrap();
    assert_eq!(natural.coverage().registry_version, 10);
    assert_eq!(natural.coverage().required_partial_bindings, 12);
    assert_eq!(natural.coverage().fully_qualified_cards, 0);
    let error = natural
        .validate_resolution_selection_source(&inventory, &source.vocabulary)
        .unwrap_err();
    assert!(matches!(
        &error,
        RegistryError::ResolutionSelectionSourceMismatch
    ));
    refuse_leaf(&inventory, &natural, &source.vocabulary, &error);
    println!("SP01 refusal separate-natural12-registry: {error:?}");

    source
        .registry
        .validate_resolution_selection_source(&inventory, &source.vocabulary)
        .unwrap();
    assert_eq!(source.registry.coverage().registry_version, 9);
    assert_eq!(source.registry.coverage().required_partial_bindings, 9);
    assert_eq!(source.registry.coverage().fully_qualified_cards, 0);
    assert!(
        materialize_prepared(
            &inventory,
            &source.registry,
            &source.vocabulary,
            &source.sources,
            &rows,
            &initialization,
        )
        .is_ok(),
        "restored genuine baseline must materialize"
    );
}
