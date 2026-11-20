//! Actual source9 Program codec bridge, without creating a game or physical card.
//! The selected source manifest supplies identity/printing joins only. Registry9
//! owns Programs, and the existing shared codec owns their complete encoding.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_card_programs::{
    BindingStatus, OracleId, RESOLUTION_SELECTION_PROFILE, RESOLUTION_SELECTION_REGISTRY_VERSION,
    Registry, Scope, TEACHING_SOURCE_SHA256,
};
use spellwake_contracts::{
    creature_types::CreatureTypeVocabulary,
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{
        AffectedPlayers, BodyTarget, CardType, Instruction, InstructionAmount, ObjectRelation,
        OrdinaryCondition, Program, ProgramKind, TriggerGrouping,
    },
    token::{TokenName, TokenRecipients, TokenSubtype},
};
use spellwake_reference_tools::{creature_vocabulary, packet, program};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::PathBuf,
};

const SOURCE_MANIFEST_SHA256: &str =
    "bc2d6e2da1415f00a72602cf5cd4bf7d24180c8b78c10748a48119f694e88c61";
const SELECTED_JSONL_SHA256: &str =
    "752106820a3213e0e8eafb92f5ab51cae3fcb18a96a405af0e69fdb7a51ab8da";
const VOCABULARY_INPUT_SHA256: &str =
    "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c";
const RAISE: &str = "5b2364d7-a811-4595-a1b4-224c70555ffa";
const FLESHBAG: &str = "4b1bf05e-753e-4350-a913-894cf3cecc0c";

fn required_path(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or_else(|| format!("explicit {name} required"))
}
fn read(path: &std::path::Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Exercise the actual raw JSON boundary and both existing decode paths. Whole
/// native equality includes names, defaults, nested token templates and triggers.
fn roundtrip(value: &Program, vocabulary: &CreatureTypeVocabulary) -> Result<Value, String> {
    let view = program::view(value);
    let bytes = serde_json::to_vec(&view).map_err(|error| error.to_string())?;
    let raw = packet::unique_json(&bytes)?;
    let owner = program::decode_for_owner(raw.clone(), vocabulary)?;
    let decoded = program::decode(raw, vocabulary)?;
    assert_eq!(&owner, value, "complete syntax-decoded native Program");
    assert_eq!(
        &decoded, value,
        "complete semantically admitted native Program"
    );
    assert_eq!(
        program::view(&decoded),
        view,
        "complete Program view roundtrip"
    );
    Ok(view)
}

/// Export the exact named Registry9 source Programs through the existing codec.
/// Stand-in input handles below do not create a game, card instance or token.
#[test]
#[ignore = "requires actual full source, original nine-source manifest, genuine vocabulary and a fresh private output path"]
fn resolution_selection_source9_programs_roundtrip_and_export() -> Result<(), String> {
    let output = required_path("SPELLWAKE_SELECTION_PROGRAM_EXPORT_OUTPUT")?;
    if output.exists() {
        return Err("selection source Program export output already exists".into());
    }
    let metadata_bytes = read(&required_path("SPELLWAKE_CATALOGUE_METADATA")?)?;
    let snapshot_bytes = read(&required_path("SPELLWAKE_CATALOGUE_INPUT")?)?;
    assert_eq!(digest(&snapshot_bytes), TEACHING_SOURCE_SHA256);
    let vocabulary_bytes = read(&required_path("SPELLWAKE_CREATURE_VOCABULARY_INPUT")?)?;
    assert_eq!(digest(&vocabulary_bytes), VOCABULARY_INPUT_SHA256);
    let vocabulary = creature_vocabulary::vocabulary(packet::unique_json(&vocabulary_bytes)?)?;
    let manifest_bytes = read(&required_path("SPELLWAKE_SELECTION_SOURCE_MANIFEST_INPUT")?)?;
    assert_eq!(manifest_bytes.len(), 10_500);
    assert_eq!(digest(&manifest_bytes), SOURCE_MANIFEST_SHA256);
    let manifest = packet::unique_json(&manifest_bytes)?;
    assert_eq!(
        manifest["kind"],
        "EXACT_NINE_SAVED_SOURCES_AND_FORGE_RESOURCE_CANDIDATES"
    );
    assert_eq!(manifest["selected_cards"]["sha256"], SELECTED_JSONL_SHA256);
    let selected = manifest["cards"]
        .as_array()
        .ok_or("original selected-nine source rows missing")?;
    assert_eq!(selected.len(), 9);
    let mut selected_by_id = BTreeMap::new();
    for row in selected {
        let oracle = row["oracle_id"]
            .as_str()
            .ok_or("source selection lacks Oracle identity")?;
        if selected_by_id.insert(oracle, row).is_some() {
            return Err("duplicate original selected Oracle identity".into());
        }
    }
    let inventory = spellwake_content::snapshot::decode(&metadata_bytes, snapshot_bytes)
        .map_err(|error| format!("actual full source intake: {error:?}"))?;
    let registry = Registry::from_resolution_selection_inventory(&inventory, &vocabulary)
        .map_err(|error| error.to_string())?;
    let coverage = registry.coverage();
    assert_eq!(
        coverage.registry_version,
        RESOLUTION_SELECTION_REGISTRY_VERSION
    );
    assert_eq!(coverage.registry_version, 9);
    assert_eq!(coverage.cards.len(), 9);
    assert_eq!(coverage.required_partial_bindings, 9);
    assert_eq!(coverage.required_without_binding, 0);
    assert_eq!(coverage.fully_qualified_cards, 0);
    let covered_ids = coverage
        .cards
        .iter()
        .map(|row| {
            row.oracle_id
                .as_deref()
                .ok_or("registry source row lacks Oracle identity")
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    assert_eq!(covered_ids, selected_by_id.keys().copied().collect());

    let mut rows = BTreeMap::new();
    let mut template_rows = Vec::new();
    let mut trigger_rows = Vec::new();
    for binding_row in &coverage.cards {
        let oracle = binding_row
            .oracle_id
            .as_deref()
            .ok_or("registry source row lacks Oracle identity")?;
        let selected = selected_by_id
            .get(oracle)
            .ok_or("registry identity is outside original nine")?;
        let mut matching = inventory
            .cards
            .iter()
            .filter(|row| row.oracle_id.as_deref() == Some(oracle));
        let source_row = matching.next().ok_or("materialized source row missing")?;
        assert!(matching.next().is_none());
        assert_eq!(source_row.scryfall_id, selected["printing_id"]);
        assert_eq!(source_row.name, selected["name"]);
        let [face] = source_row.faces.as_slice() else {
            return Err("selected source has unexpected face cardinality".into());
        };
        assert_eq!(
            face.oracle_text.as_deref(),
            selected["saved_oracle"].as_str()
        );
        let BindingStatus::Partial {
            scope,
            semantic_sha256,
            ..
        } = &binding_row.binding
        else {
            return Err("selected source has no Partial registry binding".into());
        };
        assert_eq!(*scope, Scope::ResolutionSelectionOrdinaryCardV1);
        assert_eq!(semantic_sha256.len(), 64);
        let identity = OracleId::try_from(oracle.to_owned()).map_err(|error| error.to_string())?;
        // This read accessor exposes the actual bound Program. The stand-in
        // handles are never exported or counted as physical game identities.
        let source = registry
            .seed(
                &identity,
                CardId(1),
                SeatId(41),
                ObjectDestination::LibraryBottom,
            )
            .map_err(|error| error.to_string())?
            .program;
        assert_eq!(
            source.names,
            ObjectNames::single(
                EnglishCardName::new(source_row.name.clone())
                    .map_err(|error| format!("{error:?}"))?
            )
        );
        let view = roundtrip(&source, &vocabulary)?;

        if oracle == RAISE {
            let ProgramKind::Spell(spell) = &source.kind else {
                return Err("Raise is not the actual spell Program".into());
            };
            let [Instruction::CreateTokens { creations }] = spell.body.instructions.as_slice()
            else {
                return Err("Raise lacks its complete single creation instruction".into());
            };
            let [creation] = creations.as_slice() else {
                return Err("Raise must contain exactly one authored token group".into());
            };
            assert_eq!(creation.amount, InstructionAmount::Literal(2.into()));
            assert_eq!(creation.recipients, TokenRecipients::Controller);
            let soldier = vocabulary
                .resolve_label("Soldier")
                .ok_or("genuine vocabulary lacks Soldier")?;
            assert_eq!(
                creation.definition.name(),
                &TokenName::Default {
                    ordered_subtypes: vec![TokenSubtype::Creature(soldier)]
                }
            );
            creation
                .definition
                .validate(&vocabulary)
                .map_err(|error| format!("{error:?}"))?;
            let template = creation.definition.program();
            assert!(template.names.is_empty());
            let default_view = roundtrip(template, &vocabulary)?;
            let named = creation
                .definition
                .materialize(&vocabulary)
                .map_err(|error| format!("{error:?}"))?;
            let mut expected_named = template.clone();
            expected_named.names = ObjectNames::single(
                EnglishCardName::new("Soldier Token".into())
                    .map_err(|error| format!("{error:?}"))?,
            );
            assert_eq!(
                named, expected_named,
                "name materialization changes no other complete Program value"
            );
            assert_ne!(&named, template);
            let named_view = roundtrip(&named, &vocabulary)?;
            template_rows.push(json!({
                "source_oracle_id": oracle,
                "source_instruction": 0,
                "creation_group": 0,
                "definition": program::token_definition(&creation.definition),
                "default_program": default_view,
                "materialized_named_program": named_view,
                "authored_count": "2",
                "physical_tokens_created": false,
            }));
        }
        if oracle == FLESHBAG {
            let [trigger] = source.triggered.as_slice() else {
                return Err("Fleshbag must retain its complete single trigger".into());
            };
            assert_eq!(trigger.grouping, TriggerGrouping::EachOccurrence);
            assert_eq!(
                trigger.condition,
                OrdinaryCondition::Enters(ObjectRelation::SelfObject)
            );
            assert_eq!(trigger.body.target, BodyTarget::None);
            let [
                Instruction::Sacrifice {
                    players,
                    amount,
                    filter,
                },
            ] = trigger.body.instructions.as_slice()
            else {
                return Err("Fleshbag lacks its full selected sacrifice trigger body".into());
            };
            assert_eq!(*players, AffectedPlayers::EachPlayer);
            assert_eq!(*amount, InstructionAmount::Literal(1.into()));
            assert_eq!(filter.required_types, BTreeSet::from([CardType::Creature]));
            assert!(filter.forbidden_types.is_empty());
            trigger_rows.push(json!({"source_oracle_id": oracle, "triggered_body": program::body(&trigger.body), "runtime_trigger_capture_executed": false}));
        }
        if rows.insert(oracle.to_owned(), json!({
            "source_definition": source_row,
            "registry_binding": binding_row,
            "program": view,
            "roundtrip_checks": {"unique_raw_json":true,"decode_for_owner_native_eq":true,"semantic_decode_native_eq":true,"view_eq":true},
        })).is_some() { return Err("duplicate exported source Program identity".into()); }
    }
    assert_eq!(rows.len(), 9);
    assert_eq!(template_rows.len(), 1);
    assert_eq!(trigger_rows.len(), 1);
    let document = json!({
        "schema_version": 1,
        "kind": "guarded-source9-resolution-selection-program-observation-v1",
        "profile": RESOLUTION_SELECTION_PROFILE,
        "registry_version": RESOLUTION_SELECTION_REGISTRY_VERSION,
        "catalogue_sha256": inventory.compressed_sha256,
        "catalogue_metadata": inventory.source,
        "catalogue_source_records": inventory.source_records,
        "vocabulary_input_sha256": VOCABULARY_INPUT_SHA256,
        "vocabulary": creature_vocabulary::view(&vocabulary),
        "source_manifest_sha256": SOURCE_MANIFEST_SHA256,
        "selected_jsonl_sha256": SELECTED_JSONL_SHA256,
        "registry_signature_contract": "unchanged version1 source record only; Program operands are covered by full codec/native equality, not this digest",
        "rows": rows.into_values().collect::<Vec<_>>(),
        "token_template_observations": template_rows,
        "trigger_body_observations": trigger_rows,
        "complete_program_values_roundtripped": 11,
        "fully_qualified_cards": 0,
        "limits": "Source9 codec observation only. No game, physical card/token, action, captured runtime trigger, replay, ordinary epoch, Forge comparison, seat privacy or complete-card qualification. Existing source19 exports and epochs are unchanged.",
    });
    let mut bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|error| format!("selection source Program export create: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("selection source Program export write: {error}"))?;
    drop(file);
    assert_eq!(read(&output)?, bytes, "retained export bytes");
    Ok(())
}
