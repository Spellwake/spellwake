//! Reference-only bridge from the genuine Registry8 source owner to the shared
//! sealed qualification interface. Engine test-support includes neither this
//! module nor any card-program dependency.
use crate::current_profile::{NativeOptionalRegistryWitness, registry_witness_sealed};
use serde_json::Value;
use spellwake_card_programs::Registry;

impl registry_witness_sealed::Sealed for Registry {}
impl NativeOptionalRegistryWitness for Registry {
    fn native_coverage(&self) -> Result<Value, String> {
        serde_json::to_value(self.coverage())
            .map_err(|error| format!("native Registry8 coverage serialization failed: {error}"))
    }
    fn validate_source(&self, vocabulary_id: &str) -> Result<(), String> {
        super::validate_registry_witness(self, vocabulary_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::current_profile::{
        REPORT_SCHEMA_VERSION, Recorder, RegistryWitness, SourceRef, envelope,
        validate_registry_with_owner,
    };
    use crate::projection;
    use serde_json::json;
    use spellwake_contracts::creature_types::CreatureTypeVocabulary;
    use std::collections::BTreeMap;

    fn registry_witness_value(
        witness: RegistryWitness<'_>,
        vocabulary_id: &str,
    ) -> Result<Value, String> {
        let vocabulary = crate::creature_types::selected_vocabulary()?;
        if vocabulary.id().to_string() != vocabulary_id {
            return Err("test vocabulary differs".into());
        }
        let source = |path: &str| SourceRef {
            path: path.into(),
            sha256: "0".repeat(64),
            bytes: 0,
        };
        let mut recorder = Recorder::new(
            &vocabulary,
            source("synthetic-witness-input"),
            source("synthetic-witness-publication"),
            vec![],
            witness,
        )?;
        recorder.finish_coverage(vec![], vec![], vec![], vec![])?;
        Ok(recorder.witness(&json!({}))?["registry"].clone())
    }
    // These selectors load real complete source bytes. Recorder source metadata
    // below is explicitly synthetic; these are witness/admission tests rather
    // than actual game captures.
    fn optional_registry8_from_actual_source() -> (
        super::super::OptionalSource,
        std::sync::Arc<CreatureTypeVocabulary>,
    ) {
        let metadata = std::env::var_os("SPELLWAKE_CATALOGUE_METADATA")
            .expect("explicit genuine full metadata required");
        let snapshot = std::env::var_os("SPELLWAKE_CATALOGUE_INPUT")
            .expect("explicit genuine full snapshot required");
        let vocabulary = crate::creature_types::selected_vocabulary().unwrap();
        let source = super::super::OptionalSource::load(
            std::path::Path::new(&metadata),
            std::path::Path::new(&snapshot),
            &vocabulary,
        )
        .unwrap();
        (source, vocabulary)
    }

    #[test]
    #[ignore = "requires actual full source and final compiled C03 DefinitionInput signatures; private preparation only"]
    fn registry8_witness_requires_native_owner_and_real_compiled_source19_joins() {
        let (source, vocabulary) = optional_registry8_from_actual_source();
        let registry = source.registry();
        let id = vocabulary.id().to_string();
        let coverage = serde_json::to_value(registry.coverage()).unwrap();
        assert_eq!(coverage["registryVersion"], 8);
        assert!(validate_registry_with_owner(&coverage, &id, None).is_err());
        let witness =
            registry_witness_value(RegistryWitness::OptionalPayment(registry), &id).unwrap();
        assert_eq!(witness, json!({"tag":"Registry","coverage":coverage}));
        assert!(
            registry_witness_value(RegistryWitness::OptionalPayment(registry), &"0".repeat(64))
                .is_err()
        );
    }

    #[test]
    #[ignore = "requires actual full source and final compiled C03 DefinitionInput signatures; private preparation only"]
    fn registry8_witness_rejects_foreign_source_identity_signature_scope_family_and_epoch() {
        let (source, vocabulary) = optional_registry8_from_actual_source();
        let registry = source.registry();
        let id = vocabulary.id().to_string();
        let valid = serde_json::to_value(registry.coverage()).unwrap();
        validate_registry_with_owner(&valid, &id, Some(registry)).unwrap();
        for (pointer, replacement) in [
            ("/sourceSha256", json!("0".repeat(64))),
            ("/sourceRecords", json!(39195)),
            ("/sourceRecords", json!("39196")),
            ("/requiredRecords", json!(18)),
            ("/requiredPartialBindings", json!(18)),
            ("/requiredWithoutBinding", json!(1)),
            ("/fullyQualifiedCards", json!(1)),
            ("/registryVersion", json!(7)),
            ("/registryVersion", json!(9)),
            ("/registryVersion", json!("8")),
            ("/creatureVocabularyId", json!("0".repeat(64))),
            ("/cards/0/name", json!("Foreign face")),
            (
                "/cards/0/oracleId",
                json!("00000000-0000-0000-0000-000000000001"),
            ),
            ("/cards/0/required", json!(false)),
            ("/cards/0/binding/status", json!("missing")),
            (
                "/cards/0/binding/program",
                json!("optional-payment-unknown-v1"),
            ),
            (
                "/cards/0/binding/scope",
                json!("mandatory-trigger-ordinary-card-v1"),
            ),
            ("/cards/0/binding/semanticSha256", json!("0".repeat(64))),
            ("/cards/0/binding/semanticSha256", json!(null)),
        ] {
            let mut changed = valid.clone();
            *changed.pointer_mut(pointer).unwrap() = replacement;
            assert!(
                validate_registry_with_owner(&changed, &id, Some(registry)).is_err(),
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
            assert!(validate_registry_with_owner(&changed, &id, Some(registry)).is_err());
        }
        let mut duplicate = valid.clone();
        duplicate["cards"][1]["oracleId"] = duplicate["cards"][0]["oracleId"].clone();
        assert!(validate_registry_with_owner(&duplicate, &id, Some(registry)).is_err());
        let mut missing = valid.clone();
        missing["cards"].as_array_mut().unwrap().pop();
        assert!(validate_registry_with_owner(&missing, &id, Some(registry)).is_err());
    }

    #[test]
    #[ignore = "requires actual full source and final compiled C03 DefinitionInput signatures; private preparation only"]
    fn registry8_count_preserving_key_swap_rejects_against_exact_native_owner() {
        let (source, vocabulary) = optional_registry8_from_actual_source();
        let registry = source.registry();
        let id = vocabulary.id().to_string();
        let valid = serde_json::to_value(registry.coverage()).unwrap();
        validate_registry_with_owner(&valid, &id, Some(registry)).unwrap();
        let counts = |value: &Value| {
            let mut result = BTreeMap::<String, usize>::new();
            for card in value["cards"].as_array().unwrap() {
                *result
                    .entry(card["binding"]["program"].as_str().unwrap().to_owned())
                    .or_default() += 1;
            }
            result
        };
        let other = valid["cards"]
            .as_array()
            .unwrap()
            .iter()
            .position(|card| card["binding"]["program"] != valid["cards"][0]["binding"]["program"])
            .unwrap();
        let mut changed = valid.clone();
        let first = changed["cards"][0]["binding"]["program"].clone();
        changed["cards"][0]["binding"]["program"] =
            changed["cards"][other]["binding"]["program"].clone();
        changed["cards"][other]["binding"]["program"] = first;
        assert_eq!(counts(&changed), counts(&valid));
        assert_eq!(
            validate_registry_with_owner(&changed, &id, Some(registry)).unwrap_err(),
            "Registry8 witness differs from its immutable guarded native owner"
        );
    }

    #[test]
    #[ignore = "requires actual full source and final compiled C03 DefinitionInput signatures; private preparation only"]
    fn registry8_witness_and_ordinary_envelope_admission_stay_separate() {
        let (source, vocabulary) = optional_registry8_from_actual_source();
        let registry = source.registry();
        let source_ref = |path: &str| SourceRef {
            path: path.into(),
            sha256: "0".repeat(64),
            bytes: 0,
        };
        let mut recorder = Recorder::new(
            &vocabulary,
            source_ref("synthetic-input"),
            source_ref("synthetic-publication"),
            vec![],
            RegistryWitness::OptionalPayment(registry),
        )
        .unwrap();
        recorder
            .finish_coverage(vec![], vec![], vec![], vec![])
            .unwrap();
        // Synthetic recorder metadata is an envelope unit fixture, not capture evidence.
        assert_eq!(REPORT_SCHEMA_VERSION, 10);
        let witness = recorder.witness(&json!({})).unwrap();
        assert_eq!(
            witness["schema"],
            "spellwake.current-type-context-witness.v8"
        );
        assert_eq!(
            witness["vocabulary"]["value"],
            projection::vocabulary(&vocabulary)
        );
        for profile in [
            "original",
            "copies",
            "pt",
            "prevention",
            "multipliers",
            "programs",
            "payment",
            "mana-abilities",
            "mana-color",
            "activated",
            "scheduler",
            "library",
            "opening",
            "land",
            "permanent",
            "setup",
            "declarations",
            "shared-damage",
            "cleanup",
            "ordinary",
            "teaching",
            "interaction",
            "mandatory-trigger",
        ] {
            assert_eq!(
                envelope(profile, json!({}), &recorder).unwrap_err(),
                "optional-payment envelope requires its selected Registry8 witness",
                "{profile}"
            );
        }
        for profile in ["optional-payment", "optional-payment-v1"] {
            assert!(envelope(profile, json!({}), &recorder).is_err());
        }
        // Synthetic report headers test pairing only; no game execution follows.
        let header = json!({"status":"success","data":{
            "schema_version":1,"profile":"optional-payment-v1","registry_version":8,
            "catalogue_sha256":spellwake_contracts::teaching_profile::TEACHING_SOURCE_SHA256,
            "runs":[{"schema_version":4}]
        }});
        assert!(envelope("optional-payment", header.clone(), &recorder).is_ok());
        for (pointer, changed) in [
            ("/data/schema_version", json!(2)),
            ("/data/profile", json!("mandatory-trigger-v1")),
            ("/data/registry_version", json!(7)),
            ("/data/catalogue_sha256", json!("0".repeat(64))),
            ("/data/runs/0/schema_version", json!(3)),
            ("/data/runs", json!([])),
        ] {
            let mut altered = header.clone();
            *altered.pointer_mut(pointer).unwrap() = changed;
            assert_eq!(
                envelope("optional-payment", altered, &recorder).unwrap_err(),
                "optional-payment envelope requires its ordinary Report4 header"
            );
        }
    }
}
