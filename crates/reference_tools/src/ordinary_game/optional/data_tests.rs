//! Source-input admission tests. No test runs a GameState or claims a game result.
use super::super::{
    INPUTS_SHA256, OptionalCase, OptionalSource, decode_inputs, digest, inputs,
    validate_input_shape, validate_native_templates, validate_registry_witness,
};
use super::Inputs;
use serde_json::{Value, json};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

fn compiled_value() -> Value {
    serde_json::from_slice(include_bytes!("../optional-inputs-v1.json")).unwrap()
}
#[test]
fn final_native_bound_input_and_closed_case_vocabulary_are_exact() {
    assert_eq!(
        digest(include_bytes!("../optional-inputs-v1.json")),
        INPUTS_SHA256
    );
    let input = inputs().unwrap();
    assert_eq!(input.definitions.len(), 19);
    assert_eq!(input.physical_inventory.len(), 120);
    assert_eq!(input.full_game_recipes.cases.len(), 2);
    assert_eq!(OptionalCase::ALL.len(), 26);
    assert_eq!(
        OptionalCase::FULL_GAMES,
        [OptionalCase::Fg01, OptionalCase::Fg02]
    );
    let mut ids = BTreeSet::new();
    for (index, case) in OptionalCase::ALL.into_iter().enumerate() {
        assert_eq!(OptionalCase::parse(case.as_str()).unwrap(), case);
        assert_eq!(case.game_id(), 5601 + u128::try_from(index).unwrap());
        assert_eq!(case.is_full_game(), index < 2);
        assert!(ids.insert(case.as_str()));
    }
    for unknown in [
        "", "OP-FG00", "OP-FG03", "OP-R00", "OP-R25", "FG01", "op-fg01",
    ] {
        assert!(OptionalCase::parse(unknown).is_err());
    }
}
#[test]
fn foreign_input_bytes_and_pending_signatures_have_no_fallback() {
    let mut value = compiled_value();
    value["definitions"][0]["registrySignatureV1Sha256"] = Value::Null;
    assert!(serde_json::from_value::<Inputs>(value.clone()).is_err());
    assert!(decode_inputs(&serde_json::to_vec(&value).unwrap()).is_err());
    let mut bytes = include_bytes!("../optional-inputs-v1.json").to_vec();
    bytes.push(b'\n');
    assert!(decode_inputs(&bytes).is_err());
    assert!(decode_inputs(b"{}").is_err());
}
#[test]
fn every_nested_source_boundary_rejects_unknown_and_duplicate_fields() {
    let input = compiled_value();
    for pointer in [
        "",
        "/epochs",
        "/authorities/0",
        "/definitions/0",
        "/decks/0",
        "/decks/0/entries/0",
        "/physical_inventory/0",
        "/token_templates/0",
        "/full_game_recipes",
        "/full_game_recipes/participants/0",
        "/full_game_recipes/cases/0",
        "/full_game_recipes/cases/0/decks/0",
        "/full_game_recipes/cases/0/decks/0/physical_cards/0",
        "/full_game_recipes/cases/0/turns/0",
        "/full_game_recipes/cases/0/turns/0/precombat_main_casts/0",
        "/full_game_recipes/cases/0/draw_plan",
        "/full_game_recipes/cases/0/responses_and_choices/0",
        "/full_game_recipes/cases/0/responses_and_choices/1/casting_spend",
        "/full_game_recipes/cases/0/primary_expected",
        "/full_game_recipes/cases/0/primary_expected/life",
        "/full_game_recipes/cases/0/primary_expected/resolution_payments",
        "/full_game_recipes/cases/0/primary_expected/optional_outcomes",
        "/full_game_recipes/cases/0/budgets",
    ] {
        let mut changed = input.clone();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(
            serde_json::from_value::<Inputs>(changed).is_err(),
            "unknown {pointer}"
        );
        let object = input.pointer(pointer).unwrap().as_object().unwrap();
        let (key, value) = object.iter().next().unwrap();
        let object_bytes = serde_json::to_string(object).unwrap();
        let duplicate = format!(
            "{{{}:{},{}",
            serde_json::to_string(key).unwrap(),
            serde_json::to_string(value).unwrap(),
            &object_bytes[1..]
        );
        let full = serde_json::to_string(&input).unwrap();
        assert!(full.contains(&object_bytes));
        assert!(
            serde_json::from_str::<Inputs>(&full.replacen(&object_bytes, &duplicate, 1)).is_err(),
            "duplicate {pointer}"
        );
    }
}
#[test]
fn physical_deck_order_permutation_and_entropy_shapes_reject_changes() {
    for field in [
        "epoch",
        "missing-source",
        "duplicate-source",
        "duplicate-printing",
        "duplicate-alias",
        "foreign-owner",
        "foreign-name",
        "card-order",
        "deck-count",
        "deck-source-count",
        "sideboard",
        "case-domain",
        "case-physical",
        "before-order",
        "after-permutation",
        "after-alias",
        "opening",
        "entropy-length",
        "entropy-consumed",
        "global-entropy",
        "algorithm",
        "budget",
        "opening-intent",
    ] {
        let mut input = inputs().unwrap();
        match field {
            "epoch" => input.epochs.registry = 7,
            "missing-source" => {
                input.definitions.pop();
            }
            "duplicate-source" => {
                input.definitions[1].oracle_id = input.definitions[0].oracle_id.clone()
            }
            "duplicate-printing" => {
                input.definitions[1].scryfall_id = input.definitions[0].scryfall_id.clone()
            }
            "duplicate-alias" => {
                input.physical_inventory[1].alias = input.physical_inventory[0].alias.clone()
            }
            "foreign-owner" => input.physical_inventory[0].owner = 3,
            "foreign-name" => input.physical_inventory[0].name.push('!'),
            "card-order" => input.physical_inventory[0].card_id = "99999".into(),
            "deck-count" => input.decks[0].card_count = 59,
            "deck-source-count" => input.decks[0].entries[0].count -= 1,
            "sideboard" => input.decks[0].sideboard.push(()),
            "case-domain" => input.full_game_recipes.cases[0].id = OptionalCase::R01,
            "case-physical" => input.full_game_recipes.cases[0].decks[0].physical_cards[0]
                .name
                .push('!'),
            "before-order" => input.full_game_recipes.cases[0].decks[0]
                .before_card_ids
                .swap(0, 1),
            "after-permutation" => {
                input.full_game_recipes.cases[0].decks[0].after_card_ids[0] = "99999".into()
            }
            "after-alias" => input.full_game_recipes.cases[0].decks[0]
                .after_aliases_top_first
                .swap(0, 1),
            "opening" => input.full_game_recipes.cases[0].decks[0]
                .opening_hand_aliases
                .swap(0, 1),
            "entropy-length" => {
                input.full_game_recipes.cases[0].decks[0]
                    .entropy_words
                    .pop();
            }
            "entropy-consumed" => {
                input.full_game_recipes.cases[0].decks[0].entropy_words_consumed = 58
            }
            "global-entropy" => input.full_game_recipes.cases[0].global_entropy_words[0] += 1,
            "algorithm" => input.full_game_recipes.cases[0].decks[0].algorithm = "foreign".into(),
            "budget" => input.full_game_recipes.cases[0].budgets.accepted_inputs_max += 1,
            "opening-intent" => input.full_game_recipes.opening = "prepared battlefield".into(),
            _ => unreachable!(),
        }
        assert!(validate_input_shape(&input).is_err(), "{field}");
    }
}
#[test]
fn metadata_mismatch_rejects_before_snapshot_access() {
    let directory = tempfile::tempdir().unwrap();
    let metadata = directory.path().join("metadata.json");
    std::fs::write(&metadata, b"{}").unwrap();
    let foreign = CreatureTypeVocabulary::new(
        "foreign-source-negative".into(),
        [0; 32],
        vec!["Cat".into()],
    )
    .unwrap();
    let result = OptionalSource::load(
        &metadata,
        &directory.path().join("missing-snapshot"),
        &foreign,
    );
    assert!(matches!(result, Err(error) if error == "optional metadata authority mismatch"));
}
fn actual_source() -> (OptionalSource, Arc<CreatureTypeVocabulary>) {
    let path = PathBuf::from(
        std::env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
            .expect("explicit retained source-qualified vocabulary required"),
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        digest(&bytes),
        "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c"
    );
    let vocabulary = crate::creature_vocabulary::load_vocabulary(&path).unwrap();
    let metadata = PathBuf::from(
        std::env::var_os("SPELLWAKE_CATALOGUE_METADATA")
            .expect("explicit actual full metadata required; no subset fallback"),
    );
    let snapshot = PathBuf::from(
        std::env::var_os("SPELLWAKE_CATALOGUE_INPUT")
            .expect("explicit actual full compressed catalogue required; no subset fallback"),
    );
    (
        OptionalSource::load(&metadata, &snapshot, &vocabulary).unwrap(),
        vocabulary,
    )
}
#[test]
#[ignore = "requires genuine complete catalogue/metadata/vocabulary; fixture input admission only, no GameState execution"]
fn actual_source_owner_constructs_exact_opening_inputs_and_rejects_reduced_cases() {
    let (source, vocabulary) = actual_source();
    assert_eq!(source.inventory().cards.len(), 39196);
    validate_registry_witness(source.registry(), &vocabulary.id().to_string()).unwrap();
    for case in OptionalCase::FULL_GAMES {
        let fixture = source.full_fixture(case, vocabulary.clone()).unwrap();
        assert_eq!(fixture.aliases.len(), 120);
        assert_eq!(fixture.definitions.len(), 18);
        assert_eq!(fixture.setup.seed.cards.len(), 120);
        assert!(fixture.setup.seed.mana.is_empty());
        assert!(fixture.setup.seed.initial_battlefield_order.is_empty());
        assert_eq!(fixture.setup.seed.life.len(), 2);
        assert!(
            fixture
                .setup
                .seed
                .life
                .iter()
                .all(|life| life.total == 20.into())
        );
        assert_eq!(fixture.setup.entropy.position(), 0);
        assert_eq!(fixture.setup.entropy.remaining(), 118);
        assert_eq!(fixture.shuffles.len(), 2);
        assert_eq!(
            (
                fixture.shuffles[0].entropy_start,
                fixture.shuffles[0].entropy_end
            ),
            (0, 59)
        );
        assert_eq!(
            (
                fixture.shuffles[1].entropy_start,
                fixture.shuffles[1].entropy_end
            ),
            (59, 118)
        );
    }
    for case in OptionalCase::ALL
        .into_iter()
        .filter(|case| !case.is_full_game())
    {
        assert!(source.full_fixture(case, vocabulary.clone()).is_err());
    }
    let foreign = Arc::new(
        CreatureTypeVocabulary::new("foreign".into(), [0; 32], vec!["Cat".into()]).unwrap(),
    );
    assert!(source.full_fixture(OptionalCase::Fg01, foreign).is_err());
    assert!(validate_registry_witness(source.registry(), &"0".repeat(64)).is_err());
}
#[test]
#[ignore = "requires genuine complete source; validates existing native token-template coordinates only"]
fn actual_token_reference_coordinates_reject_unknown_or_mismatched_native_bodies() {
    let (source, vocabulary) = actual_source();
    for field in [
        "unknown-source",
        "unknown-trigger",
        "wrong-kind",
        "instruction",
        "creation",
        "duplicate",
    ] {
        let mut input = source.inputs.clone();
        match field {
            "unknown-source" => {
                input.token_templates[0].source_oracle_id =
                    "00000000-0000-0000-0000-000000000001".into()
            }
            "unknown-trigger" => input.token_templates[1].triggered_ability_id = Some(999),
            "wrong-kind" => input.token_templates[1].triggered_ability_id = None,
            "instruction" => input.token_templates[0].instruction = 999,
            "creation" => input.token_templates[0].creation = 999,
            "duplicate" => input.token_templates[1] = input.token_templates[0].clone(),
            _ => unreachable!(),
        }
        assert!(
            validate_native_templates(&source.materialized, &input, &vocabulary).is_err(),
            "{field}"
        );
    }
}
