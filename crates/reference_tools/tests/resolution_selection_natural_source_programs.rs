//! Genuine saved-source admission/codec checks, not games or card qualification.
#[path = "support/natural_selection_source.rs"]
mod natural_selection_source;
use flate2::read::MultiGzDecoder;
use natural_selection_source::{digest, inputs};
use serde_json::json;
use spellwake_card_programs::{
    BindingStatus, OracleId, ProgramKey, RESOLUTION_SELECTION_NATURAL_MANA_PROFILE,
    RESOLUTION_SELECTION_NATURAL_MANA_REGISTRY_VERSION, RESOLUTION_SELECTION_PROFILE,
    RESOLUTION_SELECTION_REGISTRY_VERSION, Registry, RegistryError, Scope,
};
use spellwake_content::{CatalogFace, Inventory, Legality};
use spellwake_contracts::{
    creature_types::CreatureTypeVocabulary,
    names::{EnglishCardName, ObjectNames},
};
use spellwake_engine::{
    land::LandType,
    object_identity::{CardId, ObjectDestination},
    priority::SeatId,
    program::{CardType, PermanentDefinition, Program, ProgramKind, Supertype},
};
use spellwake_reference_tools::{packet, program};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader},
};

struct Helper {
    oracle: &'static str,
    printing: &'static str,
    name: &'static str,
    land: LandType,
    type_line: &'static str,
    text: &'static str,
    line: usize,
    raw_sha256: &'static str,
    source_sha256: &'static str,
}
const HELPERS: [Helper; 3] = [
    Helper {
        oracle: "56719f6a-1a6c-4c0a-8d21-18f7d7350b68",
        printing: "b233b117-9369-45c1-80c5-cc857386b6be",
        name: "Swamp",
        land: LandType::Swamp,
        type_line: "Basic Land — Swamp",
        text: "({T}: Add {B}.)",
        line: 13_152,
        raw_sha256: "916287ac46c937685adf05728f04157b4af7b081542042df7746e48135151dd6",
        source_sha256: "99c053ff97b10a5083056f046942b7716a7b6e882e3639e8608dee57d3f6aad4",
    },
    Helper {
        oracle: "b34bb2dc-c1af-4d77-b0b3-a0fb342a5fc6",
        printing: "2e781bfd-f8cb-4d20-b977-4359f3a21311",
        name: "Forest",
        land: LandType::Forest,
        type_line: "Basic Land — Forest",
        text: "({T}: Add {G}.)",
        line: 27_346,
        raw_sha256: "df7c145ca49a3588ef513698543d4651ab1c554161e5f2c3cc19e0937b2d9f0b",
        source_sha256: "add8a0fda94261088dc4c292b7201ebcfadb7ea46be6b3b40ce67dd7049a8fe6",
    },
    Helper {
        oracle: "bc71ebf6-2056-41f7-be35-b2e5c34afa99",
        printing: "09ac862f-0a51-44dd-b86f-9412b8218b13",
        name: "Plains",
        land: LandType::Plains,
        type_line: "Basic Land — Plains",
        text: "({T}: Add {W}.)",
        line: 28_694,
        raw_sha256: "62d89ba7d073949c430c4edbd94c2ca4f77c52e5f277048c466cb933030d12e8",
        source_sha256: "c4511685fb2da8e920d2da7ae6217aa981fb64c3670788582dee723013e0c7c6",
    },
];

fn identity(value: &str) -> OracleId {
    OracleId::try_from(value.to_owned()).unwrap()
}
fn card_index(source: &Inventory, oracle: &str) -> usize {
    source
        .cards
        .iter()
        .position(|row| row.oracle_id.as_deref() == Some(oracle))
        .unwrap()
}
fn bound(registry: &Registry, oracle: &str) -> Program {
    // Read the actual guarded Program. These handles never create a game/card.
    registry
        .seed(
            &identity(oracle),
            CardId(17),
            SeatId(9),
            ObjectDestination::Hand,
        )
        .unwrap()
        .program
}
fn expected(helper: &Helper) -> Program {
    // Independent complete value from the saved Basic Land face, not the
    // production BasicLand constructor or a clone of a teaching definition.
    Program {
        cast_timing: None,
        activation_modes: Default::default(),
        adventure: None,
        prepare_spell: None,
        enters_prepared: false,
        entry_tap_unless: None,
        opening_hand_battlefield: false,
        entry_copy: None,
        splice: None,
        nightbound_back: None,
        transform_back: None,
        modal_back: None,
        flip_back: None,
        prototype: None,
        split: None,
        cast_reductions: vec![],
        entry_counter_abilities: vec![],
        conditional_cost: None,
        plot: None,
        madness: None,
        miracle: None,
        cast_from_own_exile: false,
        exile_action: None,
        flashback: None,
        flashback_x_mana_symbols: 0,
        flashback_sacrifice_creatures: 0,
        flashback_tap_white_creatures: 0,
        flashback_life: 0,
        additional_cast_costs: vec![],
        sacrifice_creature_cast_cost: false,
        sacrifice_cast_cost_filter: None,
        tap_creatures_cast_cost: false,
        reveal_colorless_creature_cast_cost: false,
        discard_cast_cost: false,
        discard_cast_cost_filter: None,
        graveyard_creature_exile_cost: 0,
        graveyard_exile_x_cost: false,
        graveyard_self_cast: None,
        activation_restrictions: Default::default(),
        activation_hybrid_symbols: Default::default(),
        extra_activation_costs: Default::default(),
        extra_cast_costs: Vec::new(),
        loyalty_costs: Default::default(),
        variable_loyalty_costs: Default::default(),
        face_up_abilities: Vec::new(),
        printed_keywords: Default::default(),
        printed_toxic: None,
        x_mana_symbols: 0,
        hybrid_symbols: Vec::new(),
        cannot_be_countered: false,
        attack_requirement: None,
        additional_attack_requirements: Vec::new(),
        combat_limits: Default::default(),
        cast_during_own_combat: false,
        additional_life_cost: None,
        siege_back: None,
        disturb: None,
        meld_partner: None,
        printed_defense: None,
        printed_loyalty: None,
        zone_replacements: vec![],
        card_types: BTreeSet::from([CardType::Land]),
        creature_subtypes: BTreeSet::new(),
        land_subtypes: BTreeSet::from([helper.land]),
        spell_subtypes: BTreeSet::new(),
        artifact_subtypes: BTreeSet::new(),
        enchantment_subtypes: BTreeSet::new(),
        planeswalker_subtypes: Default::default(),
        names: ObjectNames::single(EnglishCardName::new(helper.name.into()).unwrap()),
        supertypes: BTreeSet::from([Supertype::Basic]),
        kind: ProgramKind::Permanent(PermanentDefinition {
            entry_tap: None,
            attachment: None,
            printed_pt: None,
            casting_cost: None,
        }),
        color_indicator: None,
        effect_defined_colors: None,
        mana: vec![],
        activated: vec![],
        triggered: vec![],
        continuous: None,
        additional_battlefield_abilities: Vec::new(),
        static_conditions: Default::default(),
        gift: None,
        casualty: None,
        conspire: false,
        dredge: None,
        assist: false,
        cipher: false,
    }
}
fn roundtrip(value: &Program, vocabulary: &CreatureTypeVocabulary) {
    let view = program::view(value);
    let raw = packet::unique_json(&serde_json::to_vec(&view).unwrap()).unwrap();
    assert_eq!(
        program::decode_for_owner(raw.clone(), vocabulary).unwrap(),
        *value
    );
    let admitted = program::decode(raw, vocabulary).unwrap();
    assert_eq!(admitted, *value);
    assert_eq!(program::view(&admitted), view);
}

#[test]
fn natural_mana_profile_identity_is_separate_from_registry9() {
    assert_eq!(
        RESOLUTION_SELECTION_PROFILE,
        "resolution-sacrifice-discard-v1"
    );
    assert_eq!(RESOLUTION_SELECTION_REGISTRY_VERSION, 9);
    assert_eq!(
        RESOLUTION_SELECTION_NATURAL_MANA_PROFILE,
        "resolution-selection-natural-mana-v1"
    );
    assert_eq!(RESOLUTION_SELECTION_NATURAL_MANA_REGISTRY_VERSION, 10);
}

#[test]
#[ignore = "requires genuine complete dated catalogue/metadata/vocabulary; no fallback"]
fn actual_natural_mana_helpers_match_full_source_values_and_shared_program_codec() {
    let (source, vocabulary, compressed) = inputs();
    let registry =
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary).unwrap();
    let coverage = registry.coverage();
    assert_eq!(coverage.registry_version, 10);
    assert_eq!(coverage.cards.len(), 12);
    assert_eq!(coverage.required_records, 12);
    assert_eq!(coverage.required_partial_bindings, 12);
    assert_eq!(coverage.required_without_binding, 0);
    assert_eq!(coverage.fully_qualified_cards, 0);
    assert_eq!(coverage.behavior_evidence, "not-evaluated-by-registry");
    for row in &coverage.cards {
        let oracle = row.oracle_id.as_deref().unwrap();
        roundtrip(&bound(&registry, oracle), &vocabulary);
    }
    let mut reader = BufReader::new(MultiGzDecoder::new(compressed.as_slice()));
    let mut line = vec![];
    let mut lines = 0;
    let mut matched = 0;
    while reader.read_until(b'\n', &mut line).unwrap() != 0 {
        lines += 1;
        if let Some(helper) = HELPERS.iter().find(|helper| helper.line == lines) {
            assert_eq!(digest(&line), helper.raw_sha256);
            let raw = packet::unique_json(&line).unwrap();
            assert_eq!(raw["oracle_id"], helper.oracle);
            assert_eq!(raw["id"], helper.printing);
            assert_eq!(raw["name"], helper.name);
            assert_eq!(raw["layout"], "normal");
            assert_eq!(raw["type_line"], helper.type_line);
            assert_eq!(raw["oracle_text"], helper.text);
            assert_eq!(raw["mana_cost"], "");
            assert_eq!(raw["legalities"]["legacy"], "legal");
            assert_eq!(raw["keywords"], json!([]));
            matched += 1;
        }
        line.clear();
    }
    assert_eq!(lines, 39_196);
    assert_eq!(matched, 3);
    for helper in &HELPERS {
        let card = &source.cards[card_index(&source, helper.oracle)];
        assert_eq!(card.scryfall_id, helper.printing);
        assert_eq!(card.layout, "normal");
        assert_eq!(
            serde_json::to_value(&card.faces).unwrap(),
            json!([{
                "name":helper.name,"type_line":helper.type_line,"oracle_text":helper.text,
                "mana_cost":"","power":null,"toughness":null,
            }])
        );
        assert_eq!(card.legalities.get("legacy"), Some(&Legality::Legal));
        let actual = bound(&registry, helper.oracle);
        assert_eq!(actual, expected(helper));
        assert!(
            actual.mana.is_empty(),
            "basic subtype supplies intrinsic mana; no duplicate printed ability"
        );
        let binding = coverage
            .cards
            .iter()
            .find(|row| row.oracle_id.as_deref() == Some(helper.oracle))
            .unwrap();
        assert!(
            matches!(&binding.binding, BindingStatus::Partial {program:ProgramKey::ResolutionSelectionBasicLandV1,scope:Scope::ResolutionSelectionOrdinaryCardV1,semantic_sha256} if semantic_sha256 == helper.source_sha256)
        );
    }
}

#[test]
#[ignore = "requires genuine complete dated catalogue/metadata/vocabulary; no fallback"]
fn actual_natural_mana_pool_preserves_all_registry9_values_and_source_signature_meaning() {
    let (source, vocabulary, _) = inputs();
    let old = Registry::from_resolution_selection_inventory(&source, &vocabulary).unwrap();
    let new =
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary).unwrap();
    assert_eq!(old.coverage().registry_version, 9);
    assert_eq!(old.coverage().cards.len(), 9);
    assert_eq!(old.coverage().required_partial_bindings, 9);
    assert_eq!(old.coverage().fully_qualified_cards, 0);
    assert_eq!(new.coverage().cards.len(), 12);
    for row in &old.coverage().cards {
        let oracle = row.oracle_id.as_deref().unwrap();
        let new_row = new
            .coverage()
            .cards
            .iter()
            .find(|other| other.oracle_id == row.oracle_id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(row).unwrap(),
            serde_json::to_value(new_row).unwrap()
        );
        assert_eq!(
            bound(&old, oracle),
            bound(&new, oracle),
            "complete original source Program {oracle}"
        );
    }
    for helper in &HELPERS {
        assert!(old.resolve(&identity(helper.oracle)).is_none());
        let record = new
            .coverage()
            .cards
            .iter()
            .find(|row| row.oracle_id.as_deref() == Some(helper.oracle))
            .unwrap();
        let before = serde_json::to_value(&record.binding).unwrap();
        let original = bound(&new, helper.oracle);
        let mut changed = original.clone();
        changed.land_subtypes.insert(LandType::Island);
        assert_ne!(changed, original);
        assert_ne!(program::view(&changed), program::view(&original));
        assert_eq!(
            serde_json::to_value(&record.binding).unwrap(),
            before,
            "source signature is not a Program-operand digest"
        );
        assert_eq!(before["semanticSha256"], helper.source_sha256);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Guard {
    Missing,
    Duplicate,
    Printing,
    Legality,
    Semantic,
}
fn helper_rejected(
    source: &Inventory,
    vocabulary: &CreatureTypeVocabulary,
    helper: &Helper,
    expected: Guard,
) {
    let error = Registry::from_resolution_selection_natural_mana_inventory(source, vocabulary)
        .err()
        .expect("corruption must reject");
    let (kind, oracle) = match error {
        RegistryError::ResolutionSelectionMissingIdentity { oracle_id } => {
            (Guard::Missing, oracle_id)
        }
        RegistryError::ResolutionSelectionDuplicateIdentity { oracle_id } => {
            (Guard::Duplicate, oracle_id)
        }
        RegistryError::ResolutionSelectionPrintingMismatch { oracle_id } => {
            (Guard::Printing, oracle_id)
        }
        RegistryError::ResolutionSelectionLegalityMismatch { oracle_id } => {
            (Guard::Legality, oracle_id)
        }
        RegistryError::ResolutionSelectionSemanticMismatch { oracle_id } => {
            (Guard::Semantic, oracle_id)
        }
        other => panic!("unexpected typed guard: {other:?}"),
    };
    assert_eq!(kind, expected);
    assert_eq!(oracle, helper.oracle);
    let old = Registry::from_resolution_selection_inventory(source, vocabulary).unwrap();
    assert_eq!(old.coverage().registry_version, 9);
    assert_eq!(
        old.coverage().cards.len(),
        9,
        "helper guards do not widen the old profile"
    );
    assert_eq!(old.coverage().required_partial_bindings, 9);
}
fn copy_face(face: &CatalogFace) -> CatalogFace {
    CatalogFace {
        name: face.name.clone(),
        type_line: face.type_line.clone(),
        oracle_text: face.oracle_text.clone(),
        mana_cost: face.mana_cost.clone(),
        power: face.power.clone(),
        toughness: face.toughness.clone(),
    }
}

#[test]
#[ignore = "requires genuine complete dated catalogue/metadata/vocabulary; mutations test guards, not source qualification"]
fn actual_natural_mana_helper_guards_reject_corruption_without_widening_registry9() {
    let (mut source, vocabulary, _) = inputs();
    let old = Registry::from_resolution_selection_inventory(&source, &vocabulary).unwrap();
    let old_ids: BTreeSet<_> = old
        .coverage()
        .cards
        .iter()
        .map(|row| row.oracle_id.as_deref().unwrap())
        .collect();
    let foreign = source
        .cards
        .iter()
        .position(|row| {
            row.oracle_id.as_deref().is_some_and(|id| {
                !old_ids.contains(id) && !HELPERS.iter().any(|helper| helper.oracle == id)
            })
        })
        .unwrap();
    for helper in &HELPERS {
        let index = card_index(&source, helper.oracle);
        let id = source.cards[index].oracle_id.take();
        helper_rejected(&source, &vocabulary, helper, Guard::Missing);
        source.cards[index].oracle_id = id;
        let id = source.cards[foreign]
            .oracle_id
            .replace(helper.oracle.into());
        helper_rejected(&source, &vocabulary, helper, Guard::Duplicate);
        source.cards[foreign].oracle_id = id;
        let printing = std::mem::replace(
            &mut source.cards[index].scryfall_id,
            "00000000-0000-0000-0000-000000000000".into(),
        );
        helper_rejected(&source, &vocabulary, helper, Guard::Printing);
        source.cards[index].scryfall_id = printing;
        let printing = std::mem::replace(
            &mut source.cards[foreign].scryfall_id,
            helper.printing.into(),
        );
        helper_rejected(&source, &vocabulary, helper, Guard::Printing);
        source.cards[foreign].scryfall_id = printing;
        let legality = source.cards[index]
            .legalities
            .insert("legacy".into(), Legality::Banned);
        helper_rejected(&source, &vocabulary, helper, Guard::Legality);
        source.cards[index]
            .legalities
            .insert("legacy".into(), legality.unwrap());
        source.cards[index].required_by_base_formats = false;
        helper_rejected(&source, &vocabulary, helper, Guard::Legality);
        source.cards[index].required_by_base_formats = true;
        let name = std::mem::replace(&mut source.cards[index].name, "changed source name".into());
        helper_rejected(&source, &vocabulary, helper, Guard::Semantic);
        source.cards[index].name = name;
        let layout = std::mem::replace(&mut source.cards[index].layout, "changed layout".into());
        helper_rejected(&source, &vocabulary, helper, Guard::Semantic);
        source.cards[index].layout = layout;
        for field in 0..6 {
            let mut replacement = copy_face(&source.cards[index].faces[0]);
            match field {
                0 => replacement.name = "changed face".into(),
                1 => replacement.type_line = Some("Land".into()),
                2 => replacement.oracle_text = Some("changed complete source clause".into()),
                3 => replacement.mana_cost = Some("{B}".into()),
                4 => replacement.power = Some("1".into()),
                5 => replacement.toughness = Some("1".into()),
                _ => unreachable!(),
            }
            let original = std::mem::replace(&mut source.cards[index].faces[0], replacement);
            helper_rejected(&source, &vocabulary, helper, Guard::Semantic);
            source.cards[index].faces[0] = original;
        }
        let faces = std::mem::take(&mut source.cards[index].faces);
        helper_rejected(&source, &vocabulary, helper, Guard::Semantic);
        source.cards[index].faces = faces;
    }
    let restored =
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary).unwrap();
    assert_eq!(restored.coverage().cards.len(), 12);
}

#[test]
#[ignore = "requires genuine complete dated catalogue/metadata/vocabulary; mutations test guards, not source qualification"]
fn actual_natural_mana_pool_keeps_full_source_envelope_and_original9_guards() {
    let (mut source, vocabulary, _) = inputs();
    let before = std::mem::replace(&mut source.compressed_sha256, "0".repeat(64));
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.compressed_sha256 = before;
    let before = std::mem::replace(&mut source.source.updated_at, "foreign".into());
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.source.updated_at = before;
    source.source_records -= 1;
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.source_records += 1;
    source.unique_oracle_ids -= 1;
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.unique_oracle_ids += 1;
    source.missing_oracle_ids = 1;
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.missing_oracle_ids = 0;
    source.duplicate_oracle_ids.push("foreign".into());
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.duplicate_oracle_ids.clear();
    source.base_format_inventory_resolved = false;
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.base_format_inventory_resolved = true;
    let removed = source.cards.pop().unwrap();
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary),
        Err(RegistryError::ResolutionSelectionSourceMismatch)
    ));
    source.cards.push(removed);
    let foreign =
        CreatureTypeVocabulary::new("foreign".into(), [0; 32], vec!["Zombie".into()]).unwrap();
    assert!(matches!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &foreign),
        Err(RegistryError::ResolutionSelectionVocabularyMismatch)
    ));
    let original_id = "058917c1-21ab-488a-9f9c-591c55f3c596";
    let index = card_index(&source, original_id);
    let id = source.cards[index].oracle_id.take();
    assert!(
        matches!(Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionMissingIdentity { oracle_id }) if oracle_id == original_id)
    );
    source.cards[index].oracle_id = id;
    let text = source.cards[index].faces[0]
        .oracle_text
        .replace("changed original9 clause".into());
    assert!(
        matches!(Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary), Err(RegistryError::ResolutionSelectionSemanticMismatch { oracle_id }) if oracle_id == original_id)
    );
    source.cards[index].faces[0].oracle_text = text;
    assert_eq!(
        Registry::from_resolution_selection_natural_mana_inventory(&source, &vocabulary)
            .unwrap()
            .coverage()
            .cards
            .len(),
        12
    );
}
