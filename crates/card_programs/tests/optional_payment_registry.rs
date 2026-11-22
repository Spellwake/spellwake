//! Proposed Registry8 content tests. Embedded provider rows are only unit inputs.
//! Actual complete source selectors remain opt-in and cannot be replaced by subsets.
use flate2::{Compression, read::MultiGzDecoder, write::GzEncoder};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_card_programs::{
    BindingStatus, OPTIONAL_PAYMENT_PROFILE, OPTIONAL_PAYMENT_REGISTRY_VERSION, OracleId,
    ProgramKey, Registry, RegistryError, Scope, TEACHING_SOURCE_SHA256, TEACHING_VOCABULARY_ID,
};
use spellwake_content::{Inventory, Legality, snapshot};
use spellwake_contracts::{
    creature_types::{CreatureTypeVocabulary, CreatureVocabularyId},
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityId, ManaCost, ManaOutput, ManaType},
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{
        BodyTarget, ExecutableBody, Instruction, PlayerRelation, Program, ResolvingPlayer,
        TriggerGrouping, TriggeredAbility, TriggeredAbilityId, ValueExpr,
    },
};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::PathBuf,
};

fn expected() -> Value {
    serde_json::from_slice(include_bytes!(
        "fixtures/optional-payment-cards.expected.json"
    ))
    .unwrap()
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn vocabulary() -> CreatureTypeVocabulary {
    let bytes = std::fs::read(
        std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
            .expect("explicit retained source-qualified vocabulary required"),
    )
    .unwrap();
    assert_eq!(digest(&bytes), expected()["creatureVocabularyInputSha256"]);
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    let hash =
        CreatureVocabularyId::try_from(raw["rules_sha256"].as_str().unwrap().to_owned()).unwrap();
    let vocabulary = CreatureTypeVocabulary::new(
        raw["rules_revision"].as_str().unwrap().to_owned(),
        *hash.as_bytes(),
        raw["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect(),
    )
    .unwrap();
    assert_eq!(vocabulary.id().to_string(), TEACHING_VOCABULARY_ID);
    vocabulary
}
fn id(value: &str) -> OracleId {
    value.to_owned().try_into().unwrap()
}
fn seed(registry: &Registry, oracle: &str) -> Program {
    registry
        .seed(
            &id(oracle),
            CardId(7),
            SeatId(1),
            ObjectDestination::LibraryBottom,
        )
        .unwrap()
        .program
}
fn new_six() -> Inventory {
    let bytes = include_bytes!("fixtures/optional-payment-new-six.jsonl.gz");
    assert_eq!(digest(bytes), expected()["sixSubsetSha256"]);
    snapshot::decode(
        include_bytes!("fixtures/optional-payment-new-six.meta.json"),
        bytes.to_vec(),
    )
    .unwrap()
}
/// Explicit synthetic post-intake unit premise. Never actual source qualification.
fn union_guard_inventory() -> Inventory {
    let mut source = new_six();
    let mut seen = source
        .cards
        .iter()
        .map(|c| c.oracle_id.clone())
        .collect::<BTreeSet<_>>();
    for other in [
        snapshot::decode(
            include_bytes!("../../../tests/catalogue/teaching-cards.meta.json"),
            include_bytes!("../../../tests/catalogue/teaching-cards.jsonl.gz").to_vec(),
        )
        .unwrap(),
        snapshot::decode(
            include_bytes!("fixtures/interaction-cards.meta.json"),
            include_bytes!("fixtures/interaction-cards.jsonl.gz").to_vec(),
        )
        .unwrap(),
        snapshot::decode(
            include_bytes!("fixtures/mandatory-cards.meta.json"),
            include_bytes!("fixtures/mandatory-cards.jsonl.gz").to_vec(),
        )
        .unwrap(),
    ] {
        for card in other.cards {
            if seen.insert(card.oracle_id.clone()) {
                source.cards.push(card);
            }
        }
    }
    assert_eq!(source.cards.len(), 46);
    source.compressed_sha256 = TEACHING_SOURCE_SHA256.into();
    source.source = serde_json::from_value(expected()["originalMetadata"].clone()).unwrap();
    source
}
fn selected_index(source: &Inventory, oracle: &str) -> usize {
    source
        .cards
        .iter()
        .position(|c| c.oracle_id.as_deref() == Some(oracle))
        .unwrap()
}
fn assert_selected(registry: &Registry) {
    let coverage = registry.coverage();
    assert_eq!(OPTIONAL_PAYMENT_PROFILE, "optional-payment-v1");
    assert_eq!(OPTIONAL_PAYMENT_REGISTRY_VERSION, 8);
    assert_eq!(coverage.registry_version, 8);
    assert_eq!(coverage.cards.len(), 19);
    assert_eq!(coverage.required_records, 19);
    assert_eq!(coverage.required_partial_bindings, 19);
    assert_eq!(coverage.required_without_binding, 0);
    assert_eq!(coverage.fully_qualified_cards, 0);
    assert_eq!(coverage.behavior_evidence, "not-evaluated-by-registry");
    let actual = coverage
        .cards
        .iter()
        .map(|c| c.oracle_id.as_deref().unwrap())
        .collect::<BTreeSet<_>>();
    let expected = expected();
    let selected = expected["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["oracleId"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, selected);
    for card in &coverage.cards {
        assert!(card.required);
        assert!(matches!(&card.binding, BindingStatus::Partial {
            scope: Scope::OptionalPaymentOrdinaryCardV1, semantic_sha256, ..
        } if semantic_sha256.len()==64));
    }
}
fn expected_new_program(name: &str, vocabulary: &CreatureTypeVocabulary) -> Program {
    let draw = Instruction::DrawController {
        amount: ValueExpr::Literal(1.into()),
    };
    let optional = Instruction::OptionalEffect {
        chooser: ResolvingPlayer::Controller,
    };
    let trigger = |condition, instructions| TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, instructions),
    };
    let creature = |subtypes: &[&str], cost| {
        Program::creature(
            Some(cost),
            1.into(),
            1.into(),
            subtypes
                .iter()
                .map(|s| vocabulary.resolve_label(s).unwrap())
                .collect(),
        )
    };
    let blue_cost = |generic| {
        let mut cost = ManaCost::one(ManaType::Blue);
        cost.generic = generic;
        cost
    };
    let mut program = match name {
        "Soul's Attendant" => {
            let mut p = creature(&["Human", "Cleric"], ManaCost::one(ManaType::White));
            p.triggered.push(trigger(
                spellwake_engine::program::TriggerSpec::enters(
                    spellwake_engine::program::ObjectFilter::creature().other_than_ability_source(),
                ),
                vec![
                    optional,
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(1.into()),
                    },
                ],
            ));
            p
        }
        "Oculus" => {
            let mut p = creature(&["Phyrexian", "Homunculus"], blue_cost(1.into()));
            p.triggered.push(trigger(
                spellwake_engine::program::TriggerSpec::dies(
                    spellwake_engine::program::ObjectFilter::self_object(),
                ),
                vec![optional, draw],
            ));
            p
        }
        "Mana Leak" => Program::instant(
            blue_cost(1.into()),
            ExecutableBody::new(
                BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::any()),
                vec![
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::TargetSpellController,
                        cost: ManaCost::generic(3.into()),
                        paid_destination: 2,
                        declined_destination: 1,
                    },
                    Instruction::CounterSpell,
                ],
            ),
        ),
        "Rhystic Study" => {
            let mut p = Program::enchantment(Some(blue_cost(2.into())));
            p.triggered.push(trigger(
                spellwake_engine::program::TriggerSpec::spell_cast(
                    PlayerRelation::Opponent,
                    spellwake_engine::program::SpellFilter::any(),
                ),
                vec![
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::UniqueRelatedPlayer,
                        cost: ManaCost::generic(1.into()),
                        paid_destination: 3,
                        declined_destination: 1,
                    },
                    optional,
                    draw,
                ],
            ));
            p
        }
        "Manalith" => {
            let mut p = Program::artifact(Some(ManaCost::generic(3.into())), vec![]);
            p.mana.push(ManaAbility {
                id: ManaAbilityId(1),
                output: ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: BTreeSet::from([
                        ManaType::White,
                        ManaType::Blue,
                        ManaType::Black,
                        ManaType::Red,
                        ManaType::Green,
                    ]),
                },
            });
            p
        }
        "Twincast" => Program::instant(
            ManaCost::specific(ManaType::Blue, 2.into()),
            ExecutableBody::new(
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
                vec![Instruction::CopySpell],
            ),
        ),
        _ => panic!("unlisted new source"),
    };
    program.names = ObjectNames::single(EnglishCardName::new(name.to_owned()).unwrap());
    program
}
#[test]
fn six_complete_source_rows_use_exact_native_constructors_roles_successors_and_subtypes() {
    let source = union_guard_inventory();
    let vocabulary = vocabulary();
    let registry = Registry::from_optional_payment_inventory(&source, &vocabulary).unwrap();
    assert_selected(&registry);
    let expected = expected();
    for card in expected["cards"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["reusedCuratedRow"] == false)
    {
        let oracle = card["oracleId"].as_str().unwrap();
        let actual = seed(&registry, oracle);
        let authored = expected_new_program(card["name"].as_str().unwrap(), &vocabulary);
        assert_eq!(actual, authored, "{}", card["name"]);
        actual.validate(&vocabulary).unwrap();
    }
}
#[test]
fn thirteen_reused_programs_and_original_twincast_keep_their_separate_profiles() {
    let source = union_guard_inventory();
    let vocabulary = vocabulary();
    let optional = Registry::from_optional_payment_inventory(&source, &vocabulary).unwrap();
    let predecessors = [
        Registry::from_teaching_inventory(&source, &vocabulary).unwrap(),
        Registry::from_interaction_inventory(&source, &vocabulary).unwrap(),
        Registry::from_mandatory_trigger_inventory(&source, &vocabulary).unwrap(),
    ];
    assert_eq!(
        predecessors
            .iter()
            .map(|r| r.coverage().cards.len())
            .collect::<Vec<_>>(),
        vec![20, 20, 19]
    );
    let expected = expected();
    let mut reused = 0;
    for card in expected["cards"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["reusedCuratedRow"] == true)
    {
        reused += 1;
        let oracle = card["oracleId"].as_str().unwrap();
        let mut found = false;
        for registry in &predecessors {
            if registry.resolve(&id(oracle)).is_some() {
                found = true;
                assert_eq!(seed(registry, oracle), seed(&optional, oracle));
            }
        }
        assert!(found);
    }
    assert_eq!(reused, 13);
    let original = Registry::from_inventory(&source, &vocabulary).unwrap();
    assert_eq!(original.coverage().registry_version, 4);
    let twincast = id(spellwake_card_programs::TWINCAST);
    assert_eq!(
        original.resolve(&twincast).unwrap().key(),
        ProgramKey::TwincastSingleTargetInstantV1
    );
    assert_eq!(
        original.resolve(&twincast).unwrap().scope(),
        Scope::SupportedInstantSpellTargetOnly
    );
    assert_eq!(
        optional.resolve(&twincast).unwrap().key(),
        ProgramKey::OptionalPaymentCopySpellV1
    );
    assert_eq!(
        seed(&original, twincast.as_str()),
        seed(&optional, twincast.as_str())
    );
    for old in predecessors {
        assert!(old.resolve(&twincast).is_none());
        for card in expected["cards"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["reusedCuratedRow"] == false)
        {
            assert!(
                old.resolve(&id(card["oracleId"].as_str().unwrap()))
                    .is_none()
            );
        }
    }
}
#[test]
fn foreign_source_metadata_vocabulary_and_unknown_identity_are_rejected() {
    let vocabulary = vocabulary();
    assert!(matches!(
        Registry::from_optional_payment_inventory(&new_six(), &vocabulary),
        Err(RegistryError::OptionalPaymentSourceMismatch)
    ));
    for field in ["hash", "object", "id", "kind", "date", "uri", "size"] {
        let mut source = union_guard_inventory();
        match field {
            "hash" => source.compressed_sha256 = "0".repeat(64),
            "object" => source.source.object = "unknown".into(),
            "id" => source.source.id = "unknown".into(),
            "kind" => source.source.kind = "default_cards".into(),
            "date" => source.source.updated_at = "2026-10-30T21:01:58.033+00:00".into(),
            "uri" => source.source.jsonl_download_uri = "https://example.invalid/source".into(),
            "size" => source.source.compressed_size += 1,
            _ => unreachable!(),
        }
        assert!(
            matches!(
                Registry::from_optional_payment_inventory(&source, &vocabulary),
                Err(RegistryError::OptionalPaymentSourceMismatch)
            ),
            "{field}"
        );
    }
    let foreign =
        CreatureTypeVocabulary::new("foreign".into(), [0; 32], vec!["Human".into()]).unwrap();
    let source = union_guard_inventory();
    assert!(matches!(
        Registry::from_optional_payment_inventory(&source, &foreign),
        Err(RegistryError::OptionalPaymentVocabularyMismatch)
    ));
    let registry = Registry::from_optional_payment_inventory(&source, &vocabulary).unwrap();
    assert!(matches!(
        registry.seed(
            &id("00000000-0000-0000-0000-000000000001"),
            CardId(99),
            SeatId(0),
            ObjectDestination::LibraryBottom
        ),
        Err(RegistryError::MissingBinding)
    ));
}
#[test]
fn all_nineteen_identity_printing_legality_and_whole_face_fields_fail_closed() {
    let vocabulary = vocabulary();
    let expected = expected();
    for card in expected["cards"].as_array().unwrap() {
        let oracle = card["oracleId"].as_str().unwrap();
        let mut source = union_guard_inventory();
        let index = selected_index(&source, oracle);
        source.cards.remove(index);
        assert!(
            matches!(Registry::from_optional_payment_inventory(&source,&vocabulary),Err(RegistryError::OptionalPaymentMissingIdentity{oracle_id}) if oracle_id==oracle)
        );
        let mut source = union_guard_inventory();
        let extra = union_guard_inventory()
            .cards
            .remove(selected_index(&source, oracle));
        source.cards.push(extra);
        assert!(
            matches!(Registry::from_optional_payment_inventory(&source,&vocabulary),Err(RegistryError::OptionalPaymentDuplicateIdentity{oracle_id}) if oracle_id==oracle)
        );
        for field in [
            "missing-oracle",
            "foreign-oracle",
            "printing",
            "duplicate-printing",
            "name",
            "layout",
            "face-name",
            "type",
            "missing-type",
            "text",
            "missing-text",
            "cost",
            "missing-cost",
            "power",
            "missing-power",
            "toughness",
            "missing-toughness",
            "extra-face",
            "missing-face",
            "legacy",
            "missing-legacy",
            "required",
        ] {
            let mut source = union_guard_inventory();
            let index = selected_index(&source, oracle);
            match field {
                "missing-oracle" => source.cards[index].oracle_id = None,
                "foreign-oracle" => {
                    source.cards[index].oracle_id =
                        Some("00000000-0000-0000-0000-000000000001".into())
                }
                "printing" => {
                    source.cards[index].scryfall_id = "00000000-0000-0000-0000-000000000001".into()
                }
                "duplicate-printing" => {
                    let mut extra = union_guard_inventory().cards.remove(index);
                    extra.oracle_id = Some("00000000-0000-0000-0000-000000000001".into());
                    source.cards.push(extra);
                }
                "name" => source.cards[index].name.push('!'),
                "layout" => source.cards[index].layout = "transform".into(),
                "face-name" => source.cards[index].faces[0].name.push('!'),
                "type" => source.cards[index].faces[0].type_line = Some("Artifact".into()),
                "missing-type" => source.cards[index].faces[0].type_line = None,
                "text" => source.cards[index].faces[0]
                    .oracle_text
                    .as_mut()
                    .unwrap()
                    .push_str("\nFlying"),
                "missing-text" => source.cards[index].faces[0].oracle_text = None,
                "cost" => source.cards[index].faces[0].mana_cost = Some("{0}".into()),
                "missing-cost" => source.cards[index].faces[0].mana_cost = None,
                "power" => source.cards[index].faces[0].power = Some("999".into()),
                "missing-power" if source.cards[index].faces[0].power.is_none() => continue,
                "missing-power" => source.cards[index].faces[0].power = None,
                "toughness" => source.cards[index].faces[0].toughness = Some("999".into()),
                "missing-toughness" if source.cards[index].faces[0].toughness.is_none() => continue,
                "missing-toughness" => source.cards[index].faces[0].toughness = None,
                "extra-face" => {
                    let extra = union_guard_inventory().cards.remove(index).faces.remove(0);
                    source.cards[index].faces.push(extra);
                }
                "missing-face" => source.cards[index].faces.clear(),
                "legacy" => {
                    source.cards[index]
                        .legalities
                        .insert("legacy".into(), Legality::Banned);
                }
                "missing-legacy" => {
                    source.cards[index].legalities.remove("legacy");
                }
                "required" => source.cards[index].required_by_base_formats = false,
                _ => unreachable!(),
            }
            // Manalith already has exactly type Artifact; the mutation must change it.
            if field == "type" && card["name"] == "Manalith" {
                source.cards[index].faces[0].type_line = Some("Enchantment".into());
            }
            let result = Registry::from_optional_payment_inventory(&source, &vocabulary);
            match field {
                "missing-oracle" | "foreign-oracle" => assert!(
                    matches!(result,Err(RegistryError::OptionalPaymentMissingIdentity{oracle_id}) if oracle_id==oracle)
                ),
                "printing" | "duplicate-printing" => assert!(
                    matches!(result,Err(RegistryError::OptionalPaymentPrintingMismatch{oracle_id}) if oracle_id==oracle)
                ),
                "legacy" | "missing-legacy" | "required" => assert!(
                    matches!(result,Err(RegistryError::OptionalPaymentLegalityMismatch{oracle_id}) if oracle_id==oracle)
                ),
                _ => assert!(
                    matches!(result,Err(RegistryError::OptionalPaymentSemanticMismatch{oracle_id}) if oracle_id==oracle),
                    "{oracle} {field}"
                ),
            }
        }
    }
}
#[test]
fn full_clauses_reject_wrong_may_unless_roles_keyword_subtype_and_color_reduction() {
    let vocabulary = vocabulary();
    for (name, text) in [
        (
            "Soul's Attendant",
            "Whenever another creature enters, you gain 1 life.",
        ),
        (
            "Soul's Attendant",
            "Whenever another creature you control enters, you may gain 1 life.",
        ),
        ("Oculus", "When this creature dies, draw a card."),
        (
            "Oculus",
            "When this creature leaves the battlefield, you may draw a card.",
        ),
        ("Mana Leak", "Counter target spell unless you pay {3}."),
        (
            "Mana Leak",
            "Counter target spell unless its controller pays {2}.",
        ),
        (
            "Rhystic Study",
            "Whenever a player casts a spell, you may draw a card unless that player pays {1}.",
        ),
        (
            "Rhystic Study",
            "Whenever an opponent casts a spell, draw a card unless that player pays {1}.",
        ),
        (
            "Rhystic Study",
            "Whenever an opponent casts a spell, you may draw a card unless you pay {1}.",
        ),
        ("Manalith", "{T}: Add {U}."),
        ("Manalith", "{T}: Add one mana of any type."),
        ("Twincast", "Copy target instant or sorcery spell."),
    ] {
        let mut source = union_guard_inventory();
        let card = source.cards.iter_mut().find(|c| c.name == name).unwrap();
        assert_ne!(card.faces[0].oracle_text.as_deref(), Some(text));
        card.faces[0].oracle_text = Some(text.into());
        assert!(
            matches!(
                Registry::from_optional_payment_inventory(&source, &vocabulary),
                Err(RegistryError::OptionalPaymentSemanticMismatch { .. })
            ),
            "{name}"
        );
    }
    let mut source = union_guard_inventory();
    source
        .cards
        .iter_mut()
        .find(|c| c.name == "Oculus")
        .unwrap()
        .faces[0]
        .type_line = Some("Creature — Homunculus".into());
    assert!(matches!(
        Registry::from_optional_payment_inventory(&source, &vocabulary),
        Err(RegistryError::OptionalPaymentSemanticMismatch { .. })
    ));
}
fn full_inputs() -> (Vec<u8>, Vec<u8>) {
    let metadata = std::fs::read(
        std::env::var_os("SPELLWAKE_CATALOGUE_METADATA")
            .expect("explicit retained full metadata required"),
    )
    .unwrap();
    let bytes = std::fs::read(
        std::env::var_os("SPELLWAKE_CATALOGUE_INPUT")
            .expect("explicit retained full source required"),
    )
    .unwrap();
    assert_eq!(digest(&metadata), expected()["originalMetadataSha256"]);
    assert_eq!(digest(&bytes), TEACHING_SOURCE_SHA256);
    (metadata, bytes)
}
#[test]
#[ignore = "requires actual complete catalogue/metadata and explicit source-qualified vocabulary; no subset fallback"]
fn actual_full_catalogue_joins_all_nineteen_raw_lines_printings_and_full_source() {
    let (metadata, bytes) = full_inputs();
    let source = snapshot::decode(&metadata, bytes.clone()).unwrap();
    assert_eq!(source.cards.len(), 39196);
    assert_eq!(source.source_records, 39196);
    let vocabulary = vocabulary();
    let registry = Registry::from_optional_payment_inventory(&source, &vocabulary).unwrap();
    assert_selected(&registry);
    let mut raw = Vec::new();
    MultiGzDecoder::new(bytes.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let lines = raw.split_inclusive(|b| *b == b'\n').collect::<Vec<_>>();
    assert_eq!(lines.len(), 39196);
    let expected = expected();
    let mut printings = BTreeSet::new();
    for card in expected["cards"].as_array().unwrap() {
        let line =
            lines[usize::try_from(card["sourceLineOneBased"].as_u64().unwrap()).unwrap() - 1];
        assert_eq!(digest(line), card["rawLineSha256IncludingLf"]);
        let raw: Value = serde_json::from_slice(line).unwrap();
        assert_eq!(raw["oracle_id"], card["oracleId"]);
        assert_eq!(raw["id"], card["printingId"]);
        assert_eq!(raw["legalities"]["legacy"], "legal");
        assert_eq!(raw["keywords"], json!([]));
        assert!(printings.insert(raw["id"].as_str().unwrap().to_owned()));
        let actual = &source.cards[selected_index(&source, card["oracleId"].as_str().unwrap())];
        assert_eq!(actual.scryfall_id, card["printingId"]);
        assert_eq!(actual.faces.len(), 1);
        assert_eq!(actual.faces[0].name, raw["name"]);
        for (field, actual) in [
            ("mana_cost", &actual.faces[0].mana_cost),
            ("type_line", &actual.faces[0].type_line),
            ("oracle_text", &actual.faces[0].oracle_text),
            ("power", &actual.faces[0].power),
            ("toughness", &actual.faces[0].toughness),
        ] {
            assert_eq!(
                actual.as_deref(),
                raw.get(field).and_then(Value::as_str),
                "{field}"
            );
        }
    }
    // Historical expectations are retained old native coverage bytes, not Registry8 results.
    let old: Value =
        serde_json::from_slice(include_bytes!("fixtures/mandatory-cards.expected.json")).unwrap();
    for (name, previous) in [
        (
            "registry4",
            Registry::from_inventory(&source, &vocabulary).unwrap(),
        ),
        (
            "registry5",
            Registry::from_teaching_inventory(&source, &vocabulary).unwrap(),
        ),
        (
            "registry6",
            Registry::from_interaction_inventory(&source, &vocabulary).unwrap(),
        ),
    ] {
        assert_eq!(
            digest(&serde_json::to_vec(previous.coverage()).unwrap()),
            old["predecessorCoverageSha256"][name]
        );
    }
    // This is the complete historical Coverage JSON value extracted by the root.
    // Its pretty JSON representation is not an original raw struct-byte claim.
    let baseline = std::fs::read(
        std::env::var_os("SPELLWAKE_OPTIONAL_REGISTRY7_BASELINE")
            .expect("explicit retained actual historical Registry7 value required"),
    )
    .unwrap();
    assert_eq!(
        digest(&baseline),
        "b1040084cb605328ac898672604f5a0bddae5f764ae6b44b4bd67b10c60f727f"
    );
    let historical: Value = serde_json::from_slice(&baseline).unwrap();
    let current = Registry::from_mandatory_trigger_inventory(&source, &vocabulary).unwrap();
    let current = serde_json::to_value(current.coverage()).unwrap();
    assert_eq!(
        current, historical,
        "whole actual Registry7 coverage value changed"
    );
    std::fs::write(
        std::env::var_os("SPELLWAKE_OPTIONAL_REGISTRY7_OBSERVED_OUTPUT")
            .expect("explicit retained current Registry7 value output path required"),
        serde_json::to_vec_pretty(&current).unwrap(),
    )
    .unwrap();
}
#[test]
#[ignore = "recompresses actual raw source mutations; explicit full inputs and lane-local output directory required"]
fn actual_full_source_raw_traits_printings_and_false_clauses_reject_at_byte_boundary() {
    let (metadata, bytes) = full_inputs();
    let vocabulary = vocabulary();
    Registry::from_optional_payment_inventory(
        &snapshot::decode(&metadata, bytes.clone()).unwrap(),
        &vocabulary,
    )
    .unwrap();
    let output = PathBuf::from(
        std::env::var_os("SPELLWAKE_OPTIONAL_CONTENT_NEGATIVE_OUTPUT")
            .expect("explicit lane-local output directory required"),
    );
    std::fs::create_dir_all(&output).unwrap();
    let mut raw = Vec::new();
    MultiGzDecoder::new(bytes.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let mut lines = raw
        .split_inclusive(|b| *b == b'\n')
        .map(<[u8]>::to_vec)
        .collect::<Vec<_>>();
    let expected = expected();
    let card = expected["cards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Rhystic Study")
        .unwrap();
    let index = usize::try_from(card["sourceLineOneBased"].as_u64().unwrap()).unwrap() - 1;
    assert_eq!(digest(&lines[index]), card["rawLineSha256IncludingLf"]);
    let original: Value = serde_json::from_slice(&lines[index]).unwrap();
    let original_line = lines[index].clone();
    let mut receipts = vec![];
    for (label, field, value) in [
        ("keyword", "keywords", json!(["Flying"])),
        ("colors", "colors", json!(["W"])),
        ("identity", "color_identity", json!(["W"])),
        ("indicator", "color_indicator", json!(["W"])),
        ("produced-mana", "produced_mana", json!(["U"])),
        (
            "printing",
            "id",
            json!("00000000-0000-0000-0000-000000000001"),
        ),
        (
            "false-clause",
            "oracle_text",
            json!("Whenever an opponent casts a spell, draw a card."),
        ),
    ] {
        let mut changed = original.clone();
        assert_ne!(changed.get(field), Some(&value));
        changed[field] = value;
        lines[index] = serde_json::to_vec(&changed).unwrap();
        lines[index].push(b'\n');
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        for line in &lines {
            encoder.write_all(line).unwrap();
        }
        let bytes = encoder.finish().unwrap();
        let mut meta: Value = serde_json::from_slice(&metadata).unwrap();
        meta["compressed_size"] = json!(bytes.len());
        let meta = serde_json::to_vec(&meta).unwrap();
        let source = snapshot::decode(&meta, bytes.clone()).unwrap();
        assert_ne!(source.compressed_sha256, TEACHING_SOURCE_SHA256);
        assert!(matches!(
            Registry::from_optional_payment_inventory(&source, &vocabulary),
            Err(RegistryError::OptionalPaymentSourceMismatch)
        ));
        std::fs::write(output.join(format!("{label}.jsonl.gz")), &bytes).unwrap();
        std::fs::write(output.join(format!("{label}.meta.json")), &meta).unwrap();
        receipts.push(json!({"mutation":label,"rawLineSha256":digest(&lines[index]),"compressedSha256":digest(&bytes),"result":"OptionalPaymentSourceMismatch"}));
        lines[index] = original_line.clone();
    }
    std::fs::write(output.join("mutation-receipts.json"),serde_json::to_vec_pretty(&json!({"kind":"ACTUAL_FULL_SOURCE_ADMISSION_NEGATIVES_NOT_GAME_QUALIFICATION","records":receipts})).unwrap()).unwrap();
}

#[test]
fn explicit_cli_profile_never_falls_back_to_the_original_registry() {
    let binary = env!("CARGO_BIN_EXE_spellwake-card-programs");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let vocabulary = std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("explicit retained source-qualified vocabulary required");
    let command = |profile| {
        std::process::Command::new(binary)
            .arg(root.join("tests/fixtures/optional-payment-new-six.meta.json"))
            .arg(root.join("tests/fixtures/optional-payment-new-six.jsonl.gz"))
            .arg("--creature-vocabulary-input")
            .arg(&vocabulary)
            .arg("--profile")
            .arg(profile)
            .output()
            .unwrap()
    };
    let unknown = command("optional-payment-v0");
    assert!(!unknown.status.success());
    let report: Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(report["status"], "fail");
    assert_eq!(report["data"]["arguments"], "invalid-input");
    let subset = command(OPTIONAL_PAYMENT_PROFILE);
    assert!(!subset.status.success());
    let report: Value = serde_json::from_slice(&subset.stdout).unwrap();
    assert_eq!(report["status"], "error");
    assert_eq!(report["code"], "registry-unavailable");
    assert!(
        String::from_utf8(subset.stderr)
            .unwrap()
            .contains("optional-payment catalogue source mismatch")
    );
}

#[test]
#[ignore = "requires actual complete source and vocabulary; exercises the canonical Registry8 CLI signature owner only after application"]
fn actual_full_source_cli_explicit_registry8_matches_the_native_coverage_owner() {
    let _ = full_inputs();
    let vocabulary = vocabulary();
    let inventory = snapshot::load(
        &PathBuf::from(std::env::var_os("SPELLWAKE_CATALOGUE_METADATA").unwrap()),
        &PathBuf::from(std::env::var_os("SPELLWAKE_CATALOGUE_INPUT").unwrap()),
    )
    .unwrap();
    let registry = Registry::from_optional_payment_inventory(&inventory, &vocabulary).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_spellwake-card-programs"))
        .arg(std::env::var_os("SPELLWAKE_CATALOGUE_METADATA").unwrap())
        .arg(std::env::var_os("SPELLWAKE_CATALOGUE_INPUT").unwrap())
        .arg("--creature-vocabulary-input")
        .arg(std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT").unwrap())
        .arg("--profile")
        .arg(OPTIONAL_PAYMENT_PROFILE)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "success");
    assert_eq!(
        report["data"],
        serde_json::to_value(registry.coverage()).unwrap()
    );
}
