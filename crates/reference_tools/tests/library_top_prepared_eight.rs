//! Exact authored native eight-case execution, in-process replay and frozen export.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_reference_tools::{
    creature_vocabulary::load_vocabulary,
    ordinary_game::library_top::{
        LibraryTopSource,
        prepared::{self, AUTHORED_INPUT},
    },
};
use std::path::PathBuf;

fn input(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("explicit genuine input required: {name}"))
}
fn source() -> LibraryTopSource {
    LibraryTopSource::load(
        &input("SPELLWAKE_CATALOGUE_METADATA"),
        &input("SPELLWAKE_CATALOGUE_INPUT"),
        load_vocabulary(&input("SPELLWAKE_CREATURE_VOCABULARY_INPUT")).unwrap(),
    )
    .unwrap()
}
fn check(report: &Value) {
    assert_eq!(
        report["schema"],
        "library-top-native-prepared-diagnostic-v2"
    );
    for action in report["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|case| case["calls"].as_array().unwrap())
        .flat_map(|call| call["result"]["library_actions"].as_array().unwrap())
    {
        assert!(action["selected_before_top_first"].is_array());
        assert!(action["public_zone_moves"].is_array());
        assert_eq!(
            action["selected_before_top_first"]
                .as_array()
                .unwrap()
                .len(),
            action["public_zone_moves"].as_array().unwrap().len()
        );
    }
    let cases = report["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    assert_eq!(report["definition_programs"].as_object().unwrap().len(), 4);
    let original: Value = serde_json::from_slice(AUTHORED_INPUT).unwrap();
    let mut total_calls = 0;
    for (ordinal, case) in cases.iter().enumerate() {
        assert_eq!(case["input_premise"], original["cases"][ordinal]);
        assert_eq!(case["case_id"], format!("LT{:02}", ordinal + 1));
        assert_eq!(case["full_native_initialization_result_replay"], true);
        assert_eq!(case["full_native_each_input_result_replay"], true);
        assert_eq!(case["full_native_final_state_replay"], true);
        let calls = case["calls"].as_array().unwrap();
        total_calls += calls.len();
        assert_eq!(
            calls
                .iter()
                .filter(|call| !call["result"]["mana_payment"].is_null())
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| !call["result"]["mana_production"].is_null())
                .count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| !call["result"]["land_play"].is_null())
                .count(),
            1
        );
        assert_eq!(case["consumed_partition_frames"], u64::from(ordinal < 4));
        assert_eq!(
            case["consumed_owner_order_frames"],
            u64::from(ordinal == 4 || ordinal == 5)
        );
        for call in calls {
            assert_eq!(call["result"]["tag"], "Ok");
        }
        assert!(case["settled_priority"]["decision"].is_null());
        assert!(case["settled_priority"]["outcome"].is_null());
        let seed = case["initialization"]["seed"]["cards"].as_array().unwrap();
        assert_eq!(
            seed.len(),
            2 + original["cases"][ordinal]["a_library_top_first"]
                .as_array()
                .unwrap()
                .len()
                + original["cases"][ordinal]["b_library_top_first"]
                    .as_array()
                    .unwrap()
                    .len()
        );
        let aliases = case["initialization"]["result"]["state"]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["alias"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(aliases.contains(&"A.LAND") && aliases.contains(&"A.SPELL"));
        if ordinal == 4 || ordinal == 5 {
            let association = &calls.last().unwrap()["authored_choice_association"];
            assert_eq!(
                association["authored_movement_order"],
                original["cases"][ordinal]["graveyard_movement_order"]
            );
            let mut reversed = association["authored_movement_order"]
                .as_array()
                .unwrap()
                .clone();
            reversed.reverse();
            assert_eq!(association["native_top_first"], json!(reversed));
            assert_eq!(
                calls.last().unwrap()["before"]["pending_work"]["tag"],
                "LibraryTop"
            );
        }
    }
    assert_eq!(total_calls, 46);
}

#[test]
#[ignore = "requires explicit genuine source inputs and a new diagnostic export directory"]
fn exact_prepared_eight_execute_replay_and_export_actual_diagnostics() {
    let output = input("SPELLWAKE_LIBRARY_TOP_DIAGNOSTIC_OUTPUT");
    assert!(!output.exists(), "diagnostic output must be new");
    let report = prepared::execute(&source(), AUTHORED_INPUT).unwrap();
    check(&report);
    // A separate genuine reopen and complete rerun must reproduce every diagnostic byte value.
    assert_eq!(
        prepared::execute(&source(), AUTHORED_INPUT).unwrap(),
        report
    );
    std::fs::create_dir(&output).unwrap();
    let mut manifest = report.clone();
    let mut pins = vec![];
    for case in report["cases"].as_array().unwrap() {
        let name = format!("{}.json", case["case_id"].as_str().unwrap());
        let bytes = serde_json::to_vec_pretty(case).unwrap();
        std::fs::write(output.join(&name), &bytes).unwrap();
        pins.push(json!({"file":name,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes))}));
    }
    manifest["cases"] = json!(pins);
    std::fs::write(
        output.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    println!(
        "exact native prepared8: cases=8 calls=46 paid_casts=8 partitions=4 owner_orders=2 complete_native_replays=8 diagnostic_files=9"
    );
}
