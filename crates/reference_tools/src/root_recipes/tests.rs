//! Codec policies and synthetic development roots. The explicit selected-current-S1 test
//! performs preparation only and never initializes a selected qualification game.
use super::{input, runner};
use crate::program;
use serde_json::json;

#[test]
fn mana_action_requires_a_tagged_origin_and_rejects_the_obsolete_scalar_field() {
    let raw = json!({"op":"activate_mana","actor":"A","source":{"current_owned_action_alias":"M"},"ability":{"tag":"BasicLand","type":"Forest"},"bind_production":null});
    assert!(serde_json::from_value::<input::Action>(raw.clone()).is_ok());
    let mut printed = raw.clone();
    printed["ability"] = json!({"tag":"Printed","id":u64::MAX});
    assert!(serde_json::from_value::<input::Action>(printed).is_ok());
    for invalid in [
        json!(1),
        json!({"tag":"BasicLand","type":"Gate"}),
        json!({"tag":"Printed","id":1,"type":"Forest"}),
    ] {
        let mut bad = raw.clone();
        bad["ability"] = invalid;
        assert!(serde_json::from_value::<input::Action>(bad).is_err());
    }
    let mut old = raw.clone();
    old.as_object_mut().unwrap().remove("ability");
    old["ability_id"] = json!(1);
    assert!(serde_json::from_value::<input::Action>(old).is_err());
    let mut mixed = raw;
    mixed["ability_id"] = json!(1);
    assert!(serde_json::from_value::<input::Action>(mixed).is_err());
}

#[test]
fn program_codec_preserves_targetless_triggered_body_and_exact_rule_integers() {
    let raw = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Creature"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":{"power":{"tag":"Number","value":"1"},"toughness":{"tag":"Number","value":"2"}},"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[{"id":7,"grouping":"EachOccurrence","condition":{"tag":"Dies","object":"SelfObject"},"body":{"target":{"tag":"None"},"instructions":[{"op":"DrawController","amount":{"tag":"Literal","value":"90071992547409931234567890"}}]}}],"continuous":null});
    let decoded = program::decode(raw.clone(), &synthetic_vocabulary()).unwrap();
    assert_eq!(program::view(&decoded), raw);
}
#[test]
fn unknown_ops_duplicate_sets_and_numeric_rule_integer_are_rejected() {
    let raw = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Instant"],"creature_subtypes":[],"kind":{"tag":"Spell","casting_cost":{"generic":"0","required":{}},"body":{"target":{"tag":"None"},"instructions":[{"op":"DrawController","amount":{"tag":"Literal","value":"1"}}]}},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null});
    let mut wrong = raw.clone();
    wrong["kind"]["body"]["instructions"][0]["op"] = json!("Whatever");
    assert!(program::decode(wrong, &synthetic_vocabulary()).is_err());
    let mut wrong = raw.clone();
    wrong["kind"]["body"]["instructions"][0]["amount"] = json!(1);
    assert!(program::decode(wrong, &synthetic_vocabulary()).is_err());
    let wrong = json!({"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Creature","Creature"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":{"power":{"tag":"Number","value":"1"},"toughness":{"tag":"Number","value":"1"}},"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null});
    assert!(program::decode(wrong, &synthetic_vocabulary()).is_err());
    let mut wrong = raw;
    wrong["kind"]["body"]["instructions"][0]["hidden"] = json!(true);
    assert!(program::decode(wrong, &synthetic_vocabulary()).is_err());
}
#[test]
fn duplicate_json_keys_are_rejected_before_case_or_definition_selection() {
    let error = parse(
        br#"{"definitions":{"unused":{"x":1,"x":2}},"fixtures":[]}"#,
        "T01",
    )
    .err()
    .unwrap();
    assert!(error.contains("duplicate object field x"));
}

fn packet() -> serde_json::Value {
    json!({"schema_version":10,"creature_vocabulary":crate::projection::vocabulary(&synthetic_vocabulary()),"kind":"deterministic-input-only-producer-recipes","no_expected_outputs":true,
        "definitions":{"intentionally_invalid_unused":{"native_program":{"kind":{"tag":"Unknown"}}}},
        "fixtures":[{"id":"T01","scenario":"T01","scope":"root_scheduled","forge_route":"descriptor_only",
            "seed":{"name_rules":[],"name_rules_id":spellwake_contracts::names::NameRules::literal_names().id().to_string(),"game_id":1,"context":["r","rules","c","p","scope"],"creature_vocabulary":synthetic_vocabulary().id().to_string(),"participants":["A","B"],"active":"A","life":{"A":"20","B":"20"},"combat_rules":"UnrestrictedFreeForAll","initial_boundary":{"kind":"Scheduled","turn":1,"step":"Upkeep"},"floating_mana":[],"cards":[],"initial_world_cohorts":[],"initial_battlefield_order":[],"library_order":{"A":[],"B":[]}},
            "actions":[],"bindings":[],"choice_tables":[],"stop":{"kind":"Ready","step":"Upkeep","turn":1,"cleanup_iteration":0,"after_action_ordinal":0,"no_extra_pass":true},"notes":[],"card_id_base":0,"engine_recipes":{"native":{"action_ordinals":[],"bindings":"native_source_receipts_and_exact_current_decisions","budget":{"max_root_inputs":512,"max_native_auto_work_returns":4096}},"forge":{}}}]})
}

#[test]
fn selected_admission_ignores_unused_invalid_definitions_but_validates_a_joined_definition() {
    let raw = packet();
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T01").unwrap();
    // Pure constructor admission only; no GameState/EffectWindow is initialized.
    assert!(selected.prepare().is_ok());
    let mut used = raw;
    used["fixtures"][0]["seed"]["cards"] = json!([{"alias":"X","card_id":9,"owner":"A","controller":null,"zone":"Hand","definition":"intentionally_invalid_unused"}]);
    let selected = parse(&serde_json::to_vec(&used).unwrap(), "T01").unwrap();
    assert!(selected.prepare().is_err());
}

#[test]
fn selected_unknown_scope_and_operation_never_coerce_into_a_root_recipe() {
    let mut raw = packet();
    raw["fixtures"][0]["scope"] = json!("anything");
    assert!(parse(&serde_json::to_vec(&raw).unwrap(), "T01").is_err());
    let mut raw = packet();
    raw["fixtures"][0]["actions"] = json!([{"op":"choose_first"}]);
    assert!(parse(&serde_json::to_vec(&raw).unwrap(), "T01").is_err());
    let mut raw = packet();
    raw["fixtures"][0]["id"] = json!("T13");
    raw["fixtures"][0]["scope"] = json!("trusted_executor");
    let error = parse(&serde_json::to_vec(&raw).unwrap(), "T13")
        .err()
        .unwrap();
    assert!(error.starts_with("unsupported_scope:"));
}

#[test]
fn bounded_recipe_is_expanded_and_input_budget_checked_without_game_execution() {
    let actions=serde_json::from_value(json!([{"op":"repeat_bounded","count":8,"sequence":[{"op":"pass_round","actors":["A","B"],"role":"resolve_exactly_one_current_top"}]}])).unwrap();
    let actions: Vec<input::Action> = actions;
    assert_eq!(runner::compile_recipe(&actions, 16).unwrap().len(), 8);
    assert!(runner::compile_recipe(&actions, 15).is_err());
    let no_work: Vec<input::Action> = serde_json::from_value(
        json!([{"op":"repeat_bounded","count":1000000000000000u64,"sequence":[]}]),
    )
    .unwrap();
    assert!(runner::compile_recipe(&no_work, 0).unwrap().is_empty());
    let duplicate:Vec<input::Action>=serde_json::from_value(json!([{"op":"repeat_bounded","count":2,"sequence":[{"op":"save_snapshot","name":"same"}]}])).unwrap();
    assert!(runner::compile_recipe(&duplicate, 0).is_err());
}

#[test]
#[ignore = "requires explicit selected current-S1 input path; pure admission, no game initialization"]
fn selected_current_s1_all61_public_inputs_admit_without_initializing_a_game() {
    use sha2::{Digest, Sha256};
    let path = std::env::var("SPELLWAKE_TRIGGER_INPUTS")
        .expect("explicit current-S1 input path is required");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        super::policy::FROZEN_INPUT_SHA256
    );
    for case in super::policy::PUBLIC_CASES {
        let selected = input::parse(&bytes, case, input::StagePolicy::OrdinaryTriggers)
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        selected
            .prepare()
            .unwrap_or_else(|error| panic!("{case}: {error}"));
        runner::compile_recipe(
            &selected.fixture.actions,
            selected
                .fixture
                .engine_recipes
                .native
                .budget
                .max_root_inputs,
        )
        .unwrap_or_else(|error| panic!("{case}: {error}"));
    }
    assert_eq!(
        super::policy::PUBLIC_CASES
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        61
    );
    let packet: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut rejected = 0;
    for fixture in packet["fixtures"].as_array().unwrap() {
        let case = fixture["id"].as_str().unwrap();
        if !super::policy::PUBLIC_CASES.contains(&case) {
            let error = input::parse(&bytes, case, input::StagePolicy::OrdinaryTriggers)
                .err()
                .unwrap();
            assert!(error.starts_with("unsupported_scope:"), "{case}: {error}");
            rejected += 1;
        }
    }
    assert_eq!(rejected, 97);
}

#[test]
fn shared_packet_prepares_private_status_and_overrides_as_data_without_applying_them() {
    use crate::packet::{DefinitionPolicy, Packet, SeedMode};
    use spellwake_engine::object_identity::{CardId, ObjectDestination};
    let mut raw = packet();
    raw["definitions"]["creature"] = json!({"native_program":{"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Creature"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":{"power":{"tag":"Number","value":"1"},"toughness":{"tag":"Number","value":"2"}},"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null}});
    let fixture = &mut raw["fixtures"][0];
    fixture["id"] = json!("private-status");
    fixture["scope"] = json!("trusted_unit_precheck");
    fixture["seed"]["initial_boundary"] = json!({"kind":"TrustedUnitPrecheck"});
    fixture["seed"]["cards"] = json!([{"alias":"X","card_id":9,"owner":"A","controller":"B","zone":"Battlefield","definition":"creature","tapped":true}]);
    fixture["seed"]["initial_battlefield_order"] = json!(["X"]);
    fixture["seed"]["trusted_precheck_overrides"] =
        json!([{"object_ref":"X","plus_one":"2","minus_one":"1"}]);
    let selected = Packet::parse(&serde_json::to_vec(&raw).unwrap())
        .unwrap()
        .select("private-status")
        .unwrap();
    assert!(
        selected
            .prepare_seed(SeedMode::PublicMain, DefinitionPolicy::ValidateSelected)
            .is_err()
    );
    let prepared = selected
        .prepare_seed(SeedMode::TrustedUnit, DefinitionPolicy::ValidateSelected)
        .unwrap();
    assert_eq!(prepared.initial_status, [(CardId(9), true)]);
    assert_eq!(prepared.aliases.alias(CardId(9)), Some("X"));
    assert_eq!(
        prepared.trusted_precheck_overrides,
        raw["fixtures"][0]["seed"]["trusted_precheck_overrides"]
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(
        prepared.seed.cards[0].destination,
        ObjectDestination::Battlefield {
            controller: crate::packet::seat("B").unwrap()
        }
    );
    // GameSeed has placement/definitions only. Applying the separate status and
    // override requests requires a real owner adapter, not the shared decoder.
    assert!(
        prepared.seed.cards[0]
            .program
            .validate(&prepared.seed.creature_vocabulary)
            .is_ok()
    );
}

#[test]
fn shared_packet_defers_only_semantic_definition_rejection_to_an_explicit_private_owner() {
    use crate::packet::{DefinitionPolicy, Packet, SeedMode};
    let mut raw = packet();
    raw["definitions"]["negative_draw"] = json!({"native_program":{"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Instant"],"creature_subtypes":[],"kind":{"tag":"Spell","casting_cost":{"generic":"0","required":{}},"body":{"target":{"tag":"None"},"instructions":[{"op":"DrawController","amount":{"tag":"Literal","value":"-1"}}]}},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null}});
    raw["fixtures"][0]["seed"]["cards"] = json!([{"alias":"N","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"negative_draw"}]);
    let select = |raw: &serde_json::Value| {
        Packet::parse(&serde_json::to_vec(raw).unwrap())
            .unwrap()
            .select("T01")
            .unwrap()
    };
    let selected = select(&raw);
    assert!(
        selected
            .prepare_seed(SeedMode::TrustedUnit, DefinitionPolicy::ValidateSelected)
            .is_err()
    );
    assert!(
        selected
            .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::DeferToOwner)
            .is_err()
    );
    let prepared = selected
        .prepare_seed(SeedMode::TrustedUnit, DefinitionPolicy::DeferToOwner)
        .unwrap();
    assert!(
        prepared.seed.cards[0]
            .program
            .validate(&prepared.seed.creature_vocabulary)
            .is_err()
    );
    raw["definitions"]["negative_draw"]["native_program"]["kind"]["body"]["instructions"][0]["op"] =
        json!("unknown");
    assert!(
        select(&raw)
            .prepare_seed(SeedMode::TrustedUnit, DefinitionPolicy::DeferToOwner)
            .is_err()
    );
}

#[test]
fn prepared_root_resources_require_explicit_mode_and_duplicate_mana_never_coalesces() {
    use crate::packet::{DefinitionPolicy, Packet, SeedMode};
    let mut raw = packet();
    raw["fixtures"][0]["seed"]["floating_mana"] =
        json!([{"player":"A","type":"Blue","amount":"3"}]);
    let select = |raw: &serde_json::Value| {
        Packet::parse(&serde_json::to_vec(raw).unwrap())
            .unwrap()
            .select("T01")
            .unwrap()
    };
    let selected = select(&raw);
    assert!(
        selected
            .prepare_seed(SeedMode::PublicMain, DefinitionPolicy::ValidateSelected)
            .is_err()
    );
    let prepared = selected
        .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
        .unwrap();
    assert_eq!(prepared.seed.mana[0].amount, 3.into());
    raw["fixtures"][0]["seed"]["floating_mana"]
        .as_array_mut()
        .unwrap()
        .push(json!({"player":"A","type":"Blue","amount":"4"}));
    assert!(
        select(&raw)
            .prepare_seed(SeedMode::PreparedRoot, DefinitionPolicy::ValidateSelected)
            .is_err()
    );
}

fn public_extension_packet(
    case: &str,
    actions: serde_json::Value,
    stop: &str,
) -> serde_json::Value {
    let mut raw = packet();
    let fixture = &mut raw["fixtures"][0];
    fixture["id"] = json!(case);
    fixture["scope"] = json!("root_unscheduled");
    fixture["seed"]["initial_boundary"] = json!({"kind":"Interaction"});
    fixture["actions"] = actions;
    let count = fixture["actions"].as_array().unwrap().len();
    fixture["stop"] = json!({"kind":stop,"after_action_ordinal":count,"no_extra_pass":true});
    fixture["engine_recipes"]["native"] = json!({"action_ordinals":(1..=count).collect::<Vec<_>>(),"budget":{"max_root_inputs":1024,"max_private_owner_calls":4096}});
    raw
}

#[test]
fn extension_syntax_rejects_private_actions_and_conflicting_copy_mutations_before_initialization() {
    let raw = public_extension_packet(
        "T16-F1",
        json!([{"op":"attempt_reply_without_adoption","mutation":{"color":"Blue","copy_choice":{"tag":"Retain","target":null}}}]),
        "Ready",
    );
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    assert!(runner::compile_recipe(&selected.fixture.actions, 1024).is_err());
    let mut raw = raw;
    raw["fixtures"][0]["actions"] =
        json!([{"op":"private_clone_set_counter","field":"next_event","value":"4"}]);
    assert!(parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").is_err());
    raw["fixtures"][0]["id"] = json!("T17-M10");
    assert!(
        parse(&serde_json::to_vec(&raw).unwrap(), "T17-M10")
            .err()
            .unwrap()
            .starts_with("unsupported_scope:")
    );
    raw["fixtures"][0]["id"] = json!("T17-M39-integrated");
    assert!(
        parse(&serde_json::to_vec(&raw).unwrap(), "T17-M39-integrated")
            .err()
            .unwrap()
            .starts_with("unsupported_scope:")
    );
}

/// Synthetic development input, separate from every frozen qualification fixture.
#[test]
fn synthetic_opening_entropy_calls_keep_two_forks_nonadopted_and_reexecute_actual_calls() {
    let mut raw = public_extension_packet(
        "T17-M40",
        json!([
            {"op":"read_only_operations","methods":["basis","pending_decision","execution","candidate.into_state_on_clone"],"repeat_count":2},
            {"op":"save_snapshot","name":"start"},
            {"op":"evaluate_same_input_on_two_forks","snapshot":"start","input":{"op":"supply_entropy","basis":"actual_snapshot_basis","words":[91,92]},"adopt_neither":true},
            {"op":"supply_entropy","basis":"actual_current_basis","words":[91,92]}
        ]),
        "PendingMulligan",
    );
    raw["definitions"]["plain"] = json!({"native_program":{"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Artifact"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":null,"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null}});
    let fixture = &mut raw["fixtures"][0];
    fixture["scope"] = json!("root_opening");
    fixture["seed"]["initial_boundary"] =
        json!({"kind":"Opening","hand_size":7,"entropy_words":vec![0u64;14]});
    let mut cards = Vec::new();
    let mut libraries = serde_json::Map::new();
    for (seat, base) in [("A", 100u64), ("B", 200u64)] {
        let mut names = Vec::new();
        for index in 0..8 {
            let name = format!("{seat}{index}");
            names.push(name.clone());
            cards.push(json!({"alias":name,"card_id":base+index,"owner":seat,"controller":null,"zone":"Library","definition":"plain"}));
        }
        libraries.insert(seat.into(), json!(names));
    }
    fixture["seed"]["cards"] = json!(cards);
    fixture["seed"]["library_order"] = json!(libraries);
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T17-M40").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
    assert_eq!(report["recipe_cursor"]["actual_root_inputs"], 3);
    assert_eq!(report["saved_input_reexecution"]["all_equal"], true);
    let records = report["records"].as_array().unwrap();
    let calls = records
        .iter()
        .filter(|record| record["tag"] == "ActualRootCall")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls
            .iter()
            .map(|row| row["adopted"].as_bool().unwrap())
            .collect::<Vec<_>>(),
        [false, false, true]
    );
    assert!(
        calls
            .iter()
            .all(|row| row["retained_source_full_state_eq"] == true
                && row["retained_current_full_state_eq"] == true
                && row["input"]["tag"] == "SupplyEntropy")
    );
    assert!(
        records
            .iter()
            .any(|row| row["tag"] == "SameInputForks" && row["full_result_eq"] == true)
    );
    assert!(records.iter().any(|row| row["tag"] == "ReadOnlyOperations"
        && row["retained_candidate_full_eq"] == true
        && row["retained_source_full_state_eq"] == true));
}

fn synthetic_copy_packet() -> serde_json::Value {
    let mut raw = public_extension_packet(
        "T16-F1",
        json!([
            {"op":"cast","actor":"A","source":{"current_owned_action_alias":"O"},"target":{"captured_object":"T"},"payment":[{"type":"Blue","amount":"1"}],"bind_entry":"original"},
            {"op":"pass","actor":"A"},
            {"op":"cast","actor":"B","source":{"current_owned_action_alias":"P"},"target":{"entry_ref":"original"},"payment":[{"type":"Blue","amount":"1"}],"bind_entry":"copy_spell"},
            {"op":"pass_round","actors":["B","A"],"role":"resolve_exactly_one_current_top"},
            {"op":"save_snapshot","name":"pending"},
            {"op":"attempt_reply_without_adoption","snapshot":"pending","mutation":{"actor":"A"}},
            {"op":"attempt_reply_without_adoption","snapshot":"pending","mutation":{"basis_revision":"actual_minus_one_checked"}},
            {"op":"reply_exact_current_frame","actor":"B","kind":"CopyTargets","items":[{"tag":"Retain","target":null}],"token":"actual_current_decision_id_and_basis"}
        ]),
        "Ready",
    );
    raw["definitions"]["target"] = json!({"native_program":{"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Creature"],"creature_subtypes":[],"kind":{"tag":"Permanent","printed_pt":{"power":{"tag":"Number","value":"2"},"toughness":{"tag":"Number","value":"4"}},"casting_cost":null},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null}});
    raw["definitions"]["original"] = json!({"native_program":{"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Instant"],"creature_subtypes":[],"kind":{"tag":"Spell","casting_cost":{"generic":"0","required":{"Blue":"1"}},"body":{"target":{"tag":"SingleCreature"},"instructions":[{"op":"Damage","amount":"1"}]}},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null}});
    raw["definitions"]["copy"] = json!({"native_program":{"names":[],"supertypes":[],"land_subtypes":[],"card_types":["Instant"],"creature_subtypes":[],"kind":{"tag":"Spell","casting_cost":{"generic":"0","required":{"Blue":"1"}},"body":{"target":{"tag":"SingleSpell","support_gate":"CopyableInstructionSpell"},"instructions":[{"op":"DrawController","amount":{"tag":"Literal","value":"0"}},{"op":"CopySpell"}]}},"color_indicator":null,"effect_defined_colors":null,"mana":[],"activated":[],"triggered":[],"continuous":null}});
    let fixture = &mut raw["fixtures"][0];
    fixture["seed"]["floating_mana"] = json!([{"player":"A","type":"Blue","amount":"3"},{"player":"B","type":"Blue","amount":"3"}]);
    fixture["seed"]["cards"] = json!([
        {"alias":"T","card_id":1,"owner":"A","controller":"A","zone":"Battlefield","definition":"target","tapped":false},
        {"alias":"O","card_id":2,"owner":"A","controller":null,"zone":"Hand","definition":"original"},
        {"alias":"P","card_id":3,"owner":"B","controller":null,"zone":"Hand","definition":"copy"}
    ]);
    fixture["seed"]["initial_battlefield_order"] = json!(["T"]);
    fixture["bindings"] = json!([{"name":"T","when":"actual_seed_receipt_or_unchanged_initialization_object","join":{"card_id":1,"require_exact_original_incarnation":true}}]);
    raw
}

#[test]
fn synthetic_copy_boundary_uses_later_literal_reply_for_negative_actor_and_checked_basis() {
    let selected = parse(
        &serde_json::to_vec(&synthetic_copy_packet()).unwrap(),
        "T16-F1",
    )
    .unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
    assert_eq!(report["saved_input_reexecution"]["all_equal"], true);
    let negatives = report["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["tag"] == "ActualInput" && row["adopt_requested"] == false)
        .collect::<Vec<_>>();
    assert_eq!(negatives.len(), 2);
    assert!(negatives.iter().all(|row| row["result"]["tag"] == "Err"
        && row["retained_source_full_state_eq"] == true
        && row["retained_current_full_state_eq"] == true));
    assert_eq!(
        negatives[0]["input"]["command"]["choice"]["tag"],
        "CopyTargets"
    );
    assert_eq!(
        negatives[0]["input"]["command"]["choice"]["choice"]["tag"],
        "Retain"
    );
}

#[test]
fn synthetic_failed_adopted_input_retains_accepted_prefix_and_raw_result_for_reexecution() {
    let mut raw = synthetic_copy_packet();
    raw["fixtures"][0]["actions"] = json!([
        {"op":"cast","actor":"A","source":{"current_owned_action_alias":"O"},"target":{"captured_object":"T"},"payment":[{"type":"Blue","amount":"1"}],"bind_entry":"original"},
        {"op":"pass","actor":"B"},
        {"op":"pass","actor":"A"}
    ]);
    raw["fixtures"][0]["engine_recipes"]["native"]["action_ordinals"] = json!([1, 2, 3]);
    raw["fixtures"][0]["stop"]["after_action_ordinal"] = json!(3);
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["recipe_cursor"]["actual_root_inputs"], 2);
    assert_eq!(report["saved_input_reexecution"]["all_equal"], true);
    assert_eq!(report["records"][1]["result"]["tag"], "Err");
    assert_eq!(report["records"][1]["adopted"], false);
    assert_eq!(report["final_state"]["stack"].as_array().unwrap().len(), 1);
}

#[test]
fn native_error_paths_match_actual_wrappers_and_preserve_typed_leaf_payload() {
    use super::projection;
    use spellwake_engine::{
        effects::EffectError, game::GameError, object_identity::ObjectIdentityError,
        stack_window::StackWindowError, turn::TurnWork,
    };
    let error = GameError::Effect(EffectError::Window(StackWindowError::Objects(
        ObjectIdentityError::IdentityExhausted,
    )));
    let projected = projection::game_error(&error);
    assert_eq!(projected["error"], format!("{error:?}"));
    assert_eq!(
        projected["error_path"],
        json!(["Effect", "Window", "Objects", "IdentityExhausted"])
    );
    assert_eq!(
        projected["error_leaf"],
        json!({"domain":"ObjectIdentityError","variant":"IdentityExhausted","data":null})
    );
    let projected = projection::game_error(&GameError::TurnWorkRequired(TurnWork::CombatDamage {
        additional: true,
    }));
    assert_eq!(projected["error_path"], json!(["TurnWorkRequired"]));
    assert_eq!(
        projected["error_leaf"]["data"],
        json!({"work":{"tag":"CombatDamage","additional":true}})
    );
}

#[test]
fn synthetic_native_initializer_error_keeps_typed_error_and_reexecutes_the_actual_result() {
    let mut raw = public_extension_packet("T16-F1", json!([]), "AfterInitialization");
    raw["fixtures"][0]["seed"]["initial_boundary"] =
        json!({"kind":"Scheduled","turn":0,"step":"PrecombatMain"});
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["initialization"]["tag"], "Err");
    assert_eq!(
        report["initialization"]["error_path"],
        json!(["Turn", "InvalidStart"])
    );
    assert_eq!(
        report["saved_input_reexecution"]["initial_full_result_eq"],
        true
    );
}

#[test]
fn synthetic_damage_attempt_observation_reuses_complete_structured_source_target_and_amount() {
    use crate::{packet, projection};
    use spellwake_engine::{
        game::{GameCommand, GameInput},
        mana::{ManaPayment, ManaSpend, ManaType},
        object_identity::CardId,
        targets::TargetRef,
    };
    let selected = parse(
        &serde_json::to_vec(&synthetic_copy_packet()).unwrap(),
        "T16-F1",
    )
    .unwrap();
    let initial = selected.initialize().unwrap().unwrap();
    let mut state = initial.into_state();
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    let target = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    let admission = state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor: packet::seat("A").unwrap(),
            command: GameCommand::CastPaid {
                spell,
                target: TargetRef::Object(target),
                payment: ManaPayment {
                    from_lots: Default::default(),
                    spend: vec![ManaSpend {
                        mana_type: ManaType::Blue,
                        amount: 1.into(),
                    }],
                },
            },
        })
        .unwrap();
    state = admission.into_state();
    for actor in ["A", "B"] {
        let result = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: packet::seat(actor).unwrap(),
                command: GameCommand::Pass,
            })
            .unwrap();
        if actor == "B" {
            let journal = result.damage_journal();
            let [attempt] = journal.attempts.as_slice() else {
                panic!("one actual attempted damage component required")
            };
            let view =
                serde_json::to_value(crate::damage_trace::damage_attempt_view(attempt)).unwrap();
            let journal_view =
                serde_json::to_value(crate::damage_trace::damage_journal_view(journal)).unwrap();
            assert_eq!(view, journal_view["attempts"][0]);
            assert_eq!(view["amount"], "1");
            assert_eq!(view["target"]["kind"], "creature");
            assert!(view["source"].is_object());
            assert!(!view["batch"].as_str().unwrap().is_empty());
            assert_eq!(
                serde_json::to_value(crate::damage_trace::damage_batch_phase_view(
                    &spellwake_engine::damage::DamageBatchPhase::Selecting
                ))
                .unwrap(),
                json!({"kind":"selecting"})
            );
            assert!(projection::candidate(&result, Some(&state), &selected.aliases).is_ok());
        }
        state = result.into_state();
    }
}

fn synthetic_vocabulary()
-> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary> {
    crate::creature_types::synthetic_vocabulary_for_rules("rules")
}

/// Synthetic adapter-only recipe. Native rule qualification uses the retained
/// source-qualified vocabulary in engine/tests/tokens.rs separately.
fn token_recipe(mana: bool) -> serde_json::Value {
    use spellwake_contracts::names::EnglishCardName;
    use spellwake_engine::{
        mana::{ManaAbility, ManaAbilityId, ManaOutput},
        program::{
            BodyTarget, ExecutableBody, Instruction, PlayerRelation, Program, TriggeredAbility,
            TriggeredAbilityId,
        },
        token::{TokenDefinition, TokenName},
    };
    let v = synthetic_vocabulary();
    let mut token = Program::artifact(None, vec![]);
    if mana {
        token.mana.push(ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: std::collections::BTreeSet::from([
                    spellwake_engine::mana::ManaType::Blue,
                    spellwake_engine::mana::ManaType::Green,
                ]),
            },
        });
    }
    let definition = TokenDefinition::new(
        token,
        TokenName::Specified(EnglishCardName::new("Fixture Token".into()).unwrap()),
        &v,
    )
    .unwrap();
    let producer = Program::instant(
        spellwake_engine::mana::ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![spellwake_engine::token::TokenCreation::one(definition)],
            }],
        ),
    );
    let mut raw = public_extension_packet(
        "T16-F1",
        json!([
            {"op":"cast","actor":"A","source":{"current_owned_action_alias":"P"},"target":null,"payment":[],"bind_entry":"producer"},
            {"op":"pass_round","actors":["A","B"],"role":"resolve_exactly_one_current_top"}
        ]),
        "Ready",
    );
    raw["definitions"]["producer"] = json!({"native_program":program::view(&producer)});
    raw["fixtures"][0]["seed"]["cards"] = json!([{"alias":"P","card_id":1,"owner":"A","controller":null,"zone":"Hand","definition":"producer"}]);
    raw["fixtures"][0]["bindings"] = json!([{"name":"token","when":"matching_actual_token_created","join":{"token_created_by_entry_ref":"producer","instruction":"0","birth_ordinal":"0","field":"after_object","require_unique":true}}]);
    if !mana {
        let mut observer = Program::artifact(None, vec![]);
        observer.triggered.push(TriggeredAbility {
            grouping: spellwake_engine::program::TriggerGrouping::EachOccurrence,
            id: TriggeredAbilityId(9),
            condition: spellwake_engine::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                spellwake_engine::program::PlayerTriggerAction::TokenCreated,
            ),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: spellwake_engine::life::LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
        raw["definitions"]["observer"] = json!({"native_program":program::view(&observer)});
        raw["fixtures"][0]["seed"]["cards"].as_array_mut().unwrap().push(json!({"alias":"W","card_id":2,"owner":"A","controller":"A","zone":"Battlefield","tapped":false,"definition":"observer"}));
        raw["fixtures"][0]["seed"]["initial_battlefield_order"] = json!(["W"]);
        raw["fixtures"][0]["bindings"].as_array_mut().unwrap().insert(0,json!({"name":"watcher","when":"after_seed_before_any_turn_work","join":{"card_id":2,"require_exact_current_incarnation":true}}));
        raw["fixtures"][0]["bindings"].as_array_mut().unwrap().push(json!({"name":"birth_discovery","when":"first_matching_actual_birth_discovery_in_this_recipe","join":{"source_object_ref":"watcher","triggered_ability_id":9,"related_token_after_ref":"token","require_unique":true}}));
    }
    raw
}
#[test]
fn token_receipts_bind_real_after_objects_and_birth_discoveries_without_card_aliases() {
    let raw = token_recipe(false);
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
    assert!(report["bindings"]["objects"]["token"].is_string());
    assert!(report["bindings"]["instances"]["birth_discovery"].is_string());
    for instruction in ["1", "999999999999999999999999999999999999"] {
        let mut bad = raw.clone();
        bad["fixtures"][0]["bindings"][1]["join"]["instruction"] = json!(instruction);
        if let Ok(selected) = parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1") {
            assert_eq!(runner::run(&selected)["status"], "failed");
        }
    }
    let mut bad = raw.clone();
    bad["fixtures"][0]["bindings"][1]["join"]["instruction"] = json!("00");
    assert!(parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1").is_err());
    // Declarations describe joins rather than an imperative binding order.
    let mut reordered = raw;
    reordered["fixtures"][0]["bindings"]
        .as_array_mut()
        .unwrap()
        .swap(1, 2);
    let selected = parse(&serde_json::to_vec(&reordered).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
}
#[test]
fn token_mana_choice_tables_bind_a_captured_object_and_reject_obsolete_aliases() {
    let mut raw = token_recipe(true);
    raw["fixtures"][0]["actions"].as_array_mut().unwrap().extend([
        json!({"op":"activate_mana","actor":"A","source":{"captured_object":"token"},"ability":{"tag":"Printed","id":7},"bind_production":null}),
        json!({"op":"attempt_reply_without_adoption","mutation":{"color":"Red"}}),
        json!({"op":"reply_exact_current_frame","actor":"A","kind":"ManaColor","items":["Blue"],"token":"actual_current_decision_id_and_basis"})]);
    raw["fixtures"][0]["stop"]["after_action_ordinal"] = json!(5);
    raw["fixtures"][0]["engine_recipes"]["native"]["action_ordinals"] = json!([1, 2, 3, 4, 5]);
    raw["fixtures"][0]["choice_tables"] = json!([{"context":"ManaColor","source":{"captured_object":"token"},"actor":"A","reply":"Blue","forge_callback":"chooseColor","allowed_color_set":["Blue","Green"],"retain_actual_offered_mask_and_order":true}]);
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
    for source in [json!({"entry_ref":"producer"}), json!({"player":"A"})] {
        let mut bad = raw.clone();
        bad["fixtures"][0]["choice_tables"][0]["source"] = source;
        assert!(parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1").is_err());
    }
    let mut bad = raw;
    bad["fixtures"][0]["choice_tables"][0]
        .as_object_mut()
        .unwrap()
        .remove("source");
    bad["fixtures"][0]["choice_tables"][0]["source_alias"] = json!("token");
    assert!(parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1").is_err());
}
fn parse(bytes: &[u8], case: &str) -> Result<input::Selected, String> {
    let mode = if super::policy::MAIN_CASES.contains(&case) {
        crate::packet::SeedMode::PublicMain
    } else if super::policy::EXTRA_PUBLIC_CASES.contains(&case) {
        crate::packet::SeedMode::PreparedRoot
    } else {
        return Err("unsupported_scope: synthetic case is outside explicit test case list".into());
    };
    input::parse_synthetic(bytes, case, mode)
}

#[test]
#[ignore = "requires explicit SPELLWAKE_RESOLUTION_CODEC_INPUT_DIR with the selected current raw candidates; invokes actual initializers only, no recipe actions"]
fn selected_raw_context_mismatches_reach_native_admission_and_positive_initialization() {
    use spellwake_engine::game::GameError;
    let root = std::path::PathBuf::from(
        std::env::var_os("SPELLWAKE_RESOLUTION_CODEC_INPUT_DIR")
            .expect("explicit sealed raw input directory required"),
    );
    for name in [
        "S2C-VM30-051.packet.candidate.json",
        "S2C-VM30-052.packet.candidate.json",
    ] {
        let bytes = std::fs::read(root.join(name)).unwrap();
        let selected =
            input::parse(&bytes, "S2R01-DM", input::StagePolicy::ResolutionValues).unwrap();
        assert_eq!(
            selected.initialize().unwrap(),
            Err(GameError::InvalidContext),
            "{name}"
        );
    }
    let bytes = std::fs::read(root.join("baseline.packet.candidate.json")).unwrap();
    let selected = input::parse(&bytes, "S2R01-DM", input::StagePolicy::ResolutionValues).unwrap();
    let candidate = selected.initialize().unwrap().unwrap();
    assert!(candidate.state().pending_decision().is_none());
    assert_eq!(
        candidate.state().execution().creature_vocabulary(),
        selected.vocabulary.as_ref()
    );
    assert!(input::parse(&bytes, "S2R-unlisted", input::StagePolicy::ResolutionValues).is_err());
}

#[test]
fn token_plural_recipe_uses_actual_order_frame_and_selects_the_second_actual_birth() {
    // Synthetic adapter data: execution of real root/codec/join paths, not card qualification.
    let mut raw = token_recipe(true);
    raw["definitions"]["producer"]["native_program"]["kind"]["body"]["instructions"][0]["creations"]
        [0]["amount"] = json!({"tag":"Literal","value":"2"});
    raw["fixtures"][0]["bindings"][0]["join"]["birth_ordinal"] = json!("1");
    raw["fixtures"][0]["actions"].as_array_mut().unwrap().push(json!({"op":"reply_exact_current_frame","actor":"A","kind":"TimestampOrder","items":[1,0],"token":"actual_current_decision_id_and_basis"}));
    raw["fixtures"][0]["stop"]["after_action_ordinal"] = json!(3);
    raw["fixtures"][0]["engine_recipes"]["native"]["action_ordinals"] = json!([1, 2, 3]);
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
    assert!(report["bindings"]["objects"]["token"].is_string());
    for value in [json!("2"), json!("99")] {
        let mut bad = raw.clone();
        bad["fixtures"][0]["bindings"][0]["join"]["birth_ordinal"] = value;
        let selected = parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1").unwrap();
        assert_eq!(runner::run(&selected)["status"], "failed");
    }
    for value in [json!(1), json!("01")] {
        let mut bad = raw.clone();
        bad["fixtures"][0]["bindings"][0]["join"]["birth_ordinal"] = value;
        assert!(parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1").is_err());
    }
    let mut bad = raw;
    bad["fixtures"][0]["actions"][2]["items"] = json!(["1", "0"]);
    // Reply items are raw Value at structural decode. The single recipe compiler
    // validates typed leaf grammar before initialization or any native input.
    let selected = parse(&serde_json::to_vec(&bad).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "failed");
    assert!(
        report["failure"]
            .as_str()
            .unwrap()
            .contains("timestamp member")
    );
}

#[test]
fn grouped_birth_joins_bind_either_actual_member_to_one_instance_and_preserve_binding_wire() {
    let mut raw = token_recipe(false);
    raw["definitions"]["producer"]["native_program"]["kind"]["body"]["instructions"][0]["creations"]
        [0]["amount"] = json!({"tag":"Literal","value":"2"});
    raw["definitions"]["observer"]["native_program"]["triggered"][0]["grouping"] =
        json!("OneOrMoreTogether");
    raw["fixtures"][0]["actions"].as_array_mut().unwrap().push(json!({"op":"reply_exact_current_frame","actor":"A","kind":"TimestampOrder","items":[1,0],"token":"actual_current_decision_id_and_basis"}));
    raw["fixtures"][0]["stop"]["after_action_ordinal"] = json!(3);
    raw["fixtures"][0]["engine_recipes"]["native"]["action_ordinals"] = json!([1, 2, 3]);
    let bindings = raw["fixtures"][0]["bindings"].as_array_mut().unwrap();
    bindings.push(json!({"name":"second_token","when":"matching_actual_token_created","join":{"token_created_by_entry_ref":"producer","instruction":"0","birth_ordinal":"1","field":"after_object","require_unique":true}}));
    bindings.push(json!({"name":"second_discovery","when":"first_matching_actual_birth_discovery_in_this_recipe","join":{"source_object_ref":"watcher","triggered_ability_id":9,"related_token_after_ref":"second_token","require_unique":true}}));
    let selected = parse(&serde_json::to_vec(&raw).unwrap(), "T16-F1").unwrap();
    let report = runner::run(&selected);
    assert_eq!(report["status"], "completed_recipe", "{report}");
    assert_eq!(
        report["bindings"]["instances"]["birth_discovery"],
        report["bindings"]["instances"]["second_discovery"]
    );
    let stack = report["final_state"]["stack"].as_array().unwrap();
    assert_eq!(stack.len(), 1);
    let binding = &stack[0]["bindings"]["trigger"];
    assert_eq!(
        binding["instance"],
        report["bindings"]["instances"]["birth_discovery"]
    );
    assert_eq!(binding["causes"]["key"], json!({"tag":"Together"}));
    let members = binding["causes"]["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    for member in members {
        assert_eq!(member["related_player"], 0);
        assert_eq!(member["occurrence"]["event"], binding["causes"]["event"]);
    }
    assert_ne!(members[0]["occurrence"], members[1]["occurrence"]);
    assert!(binding.get("occurrence").is_none() && binding.get("related").is_none());
    assert!(
        report["final_state"]["waiting"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn strict_grouping_and_count_codec_round_trip_and_reject_missing_or_unknown_fields() {
    let mut raw = token_recipe(false)["definitions"]["observer"]["native_program"].clone();
    for grouping in ["EachOccurrence", "OneOrMoreTogether", "OneOrMoreByPlayer"] {
        raw["triggered"][0]["grouping"] = json!(grouping);
        raw["triggered"][0]["body"]["instructions"] =
            json!([{"op":"DrawController","amount":{"tag":"MatchedOccurrences"}}]);
        let decoded = program::decode(raw.clone(), &synthetic_vocabulary()).unwrap();
        assert_eq!(program::view(&decoded), raw);
        decoded.validate(&synthetic_vocabulary()).unwrap();
    }
    let mut missing = raw.clone();
    missing["triggered"][0]
        .as_object_mut()
        .unwrap()
        .remove("grouping");
    assert!(program::decode(missing, &synthetic_vocabulary()).is_err());
    for wrong in [json!(1), json!(null), json!({"OneOrMoreTogether":null})] {
        let mut malformed = raw.clone();
        malformed["triggered"][0]["grouping"] = wrong;
        assert!(program::decode(malformed, &synthetic_vocabulary()).is_err());
    }
    raw["triggered"][0]["grouping"] = json!("OncePerTurn");
    assert!(program::decode(raw.clone(), &synthetic_vocabulary()).is_err());
    raw["triggered"][0]["grouping"] = json!("OneOrMoreTogether");
    raw["triggered"][0]["body"]["instructions"][0]["amount"]["fallback"] = json!(1);
    assert!(program::decode(raw, &synthetic_vocabulary()).is_err());
}
