//! Opt-in private qualification caller over actual native owners and input-only data.
use super::*;
use crate::{
    characteristics::PowerToughnessCounters,
    combat::{AttackDeclaration, BlockDeclaration},
    effects::private_qualification::QualificationFault,
    mana::ManaSpend,
    object_identity::{CardId, ObjectDestination, ObjectMovement},
    occurrence::{RulesRecord, TriggerError, TriggerInstanceId, TriggerRelated},
    test_support::{creature_types, packet, program, projection},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::Path};

const INPUT_SHA256: &str = "16dc72842bcd3b5bd7cda1e77bd45491cf67dccac3641ca91f4ef9befd2e80a6";

mod capture;
mod input;
mod root;
mod tests;
mod token_batch_tests;
mod trusted;
mod v2;
use input::{Joins, command, integer, quantity, reply, rows, string};

#[derive(Debug, PartialEq, Eq)]
enum Witness {
    Root {
        initialization: Box<Result<NativeCandidate, GameError>>,
        attempts: Vec<(GameState, GameInput, Result<NativeCandidate, GameError>)>,
        final_state: Option<Box<GameState>>,
    },
    Moves {
        source: Box<EffectWindow>,
        result: Box<
            Result<
                crate::occurrence::RulesTransition<EffectWindow, Vec<ObjectMovement>>,
                EffectError,
            >,
        >,
    },
    Checks {
        seeded: Box<EffectWindow>,
        prepared: Box<EffectWindow>,
        result: Box<Result<RuleProgress, EffectError>>,
    },
    Index(Result<(u64, u64), TriggerError>),
    Shape(Result<(), TriggerError>),
    EffectInitialization(Box<Result<EffectWindow, EffectError>>),
    EffectPreparation {
        seeded: Box<EffectWindow>,
        source: Box<EffectWindow>,
        result: Result<(), EffectError>,
        retained: Box<EffectWindow>,
    },
}

fn execute(selected: &packet::SelectedInput, case: &str) -> Result<(Value, Witness), String> {
    match selected.fixture["scope"].as_str().ok_or("missing scope")? {
        "native_only_private_fault_at_reached_owner_state" if case.starts_with("T18-") => {
            root::run(selected)
        }
        "trusted_executor" | "trusted_executor_precheck" if matches!(case, "T13" | "T14") => {
            trusted::run(selected, case)
        }
        "pure_checked_index_unit" => {
            let [action] = rows(&selected.fixture, "actions")? else {
                return Err("pure index requires one call".into());
            };
            if string(action, "op")? != "pure_checked_occurrence_index_step"
                || action["allocate_vector"] != false
                || !selected.fixture["seed"].is_null()
            {
                return Err("wrong pure arithmetic input".into());
            }
            let index = integer(&action["starting_index"])?;
            let saved_index = index;
            let operands = json!({"index":index.to_string()});
            let result = capture::pure(
                "next_occurrence_index",
                &operands,
                || crate::occurrence::next_occurrence_index(index),
                |result| {
                    Ok(match result {
                        Ok((allocated, next)) => {
                            json!({"tag":"Ok","allocated":allocated.to_string(),"next":next.to_string()})
                        }
                        Err(error) => projection::trigger_error(error),
                    })
                },
                || index == saved_index,
            )?;
            let view = match &result {
                Ok((allocated, next)) => {
                    json!({"tag":"Ok","allocated":allocated.to_string(),"next":next.to_string()})
                }
                Err(error) => projection::trigger_error(error),
            };
            let mut report = json!({"status":"completed_recipe","scope":"pure_checked_index_unit","input":action,"result":view,"game_candidate_claim":false,"vector_allocation":false});
            if crate::test_support::current_capture::is_active() {
                let point = crate::test_support::current_capture::observation(
                    "actual pure checked index result",
                    crate::test_support::current_profile::SnapshotOwner::PureValue(&view),
                    &packet::AliasContext::default(),
                )?;
                report["__current_capture_point"] = json!(point);
            }
            Ok((report, Witness::Index(result)))
        }
        "pure_event_shape_unit" => {
            let [action] = rows(&selected.fixture, "actions")? else {
                return Err("pure shape requires one call".into());
            };
            if string(action, "op")? != "pure_validate_event_shape"
                || !rows(action, "occurrences")?.is_empty()
                || action["publish"] != false
                || !selected.fixture["seed"].is_null()
            {
                return Err("unsupported pure shape input".into());
            }
            let occurrences: [crate::occurrence::RulesOccurrence; 0] = [];
            let saved = occurrences.clone();
            let operands = json!({"occurrences":[]});
            let result = capture::pure(
                "validate_event_shape",
                &operands,
                || crate::occurrence::validate_event_shape(&occurrences),
                |result| {
                    Ok(match result {
                        Ok(()) => json!({"tag":"Ok"}),
                        Err(error) => projection::trigger_error(error),
                    })
                },
                || occurrences == saved,
            )?;
            let view = match &result {
                Ok(()) => json!({"tag":"Ok"}),
                Err(error) => projection::trigger_error(error),
            };
            let mut report = json!({"status":"completed_recipe","scope":"pure_event_shape_unit","input":action,"result":view,"game_candidate_claim":false,"published_event":false});
            if crate::test_support::current_capture::is_active() {
                let point = crate::test_support::current_capture::observation(
                    "actual pure event shape result",
                    crate::test_support::current_profile::SnapshotOwner::PureValue(&view),
                    &packet::AliasContext::default(),
                )?;
                report["__current_capture_point"] = json!(point);
            }
            Ok((report, Witness::Shape(result)))
        }
        scope => Err(format!(
            "unsupported_scope: private profile does not yet execute {case}/{scope}"
        )),
    }
}

fn produce(path: &Path, case: &str) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let observed = (|| -> Result<Value, String> {
        if digest != INPUT_SHA256 {
            return Err("private input bytes differ from selected B08 freeze".into());
        }
        let packet = packet::Packet::parse(&bytes)?;
        let selected = packet.select(case)?;
        let explicit =
            creature_types::load_vocabulary(&creature_types::selected_vocabulary_path()?)?;
        if selected.vocabulary != explicit {
            return Err("selected packet vocabulary differs from explicit capture input".into());
        }
        capture::begin(path, "ordinary-trigger-private-unit", None)?;
        let first = execute(&selected, case);
        let second =
            crate::test_support::current_capture::without_capture(|| execute(&selected, case));
        let typed_eq = match (&first, &second) {
            (Ok((_, first)), Ok((_, second))) => Some(first == second),
            _ => None,
        };
        let mut first_view = match first {
            Ok((view, _)) => view,
            Err(error) => {
                json!({"status":"failed","failure_domain":"DriverOrCodec","failure":error})
            }
        };
        let second_view = match second {
            Ok((view, _)) => view,
            Err(error) => {
                json!({"status":"failed","failure_domain":"DriverOrCodec","failure":error})
            }
        };
        capture::bind(&mut first_view)?;
        let record_eq = first_view == second_view;
        Ok(
            json!({"selected_input":selected.fixture,"selected_definitions":selected.definitions,"packet_metadata":selected.metadata,"observation":first_view,"reexecution":{"full_native_result_and_state_eq":typed_eq,"full_record_eq":record_eq},"native_reexecution_integrity":typed_eq==Some(true)&&record_eq}),
        )
    })();
    // Preserve the actual read binding even if packet decoding or driver admission
    // fails. A second read is a stability observation, never a substitute input.
    let after = fs::read(path);
    let after_digest = after
        .as_ref()
        .ok()
        .map(|value| format!("{:x}", Sha256::digest(value)));
    let unchanged = after.as_ref().is_ok_and(|value| value == &bytes);
    let mut report = match observed {
        Ok(value) => value,
        Err(error) => {
            json!({"observation":{"status":"failed","failure_domain":"DriverOrCodec","failure":error},"native_reexecution_integrity":false})
        }
    };
    report["schema_version"] = json!(crate::test_support::current_profile::REPORT_SCHEMA_VERSION);
    report["record_codec"] = json!("spellwake.current-native-qualification.v2");
    report["profile"] = json!("ordinary-trigger-private-unit");
    report["case"] = json!(case);
    report["input_source"] = json!({"path":path,"sha256_read":digest,"bytes_read":bytes.len(),"parser_called":digest==INPUT_SHA256,"sha256_parser_input":(digest==INPUT_SHA256).then_some(&digest),"sha256_after":after_digest,"after_read_error":after.as_ref().err().map(ToString::to_string),"unchanged":unchanged});
    report["producer_integrity"] =
        json!(report["native_reexecution_integrity"] == true && unchanged);
    report["status"] = json!(if report["producer_integrity"] == true
        && report["observation"]["status"] == "completed_recipe"
    {
        "completed_recipe"
    } else {
        "failed"
    });
    report["qualification_claim"] = json!(
        "none; independent expected checking and captured executable/source/input evidence required"
    );
    capture::finish("ordinary-trigger-private-unit", &mut report);
    Ok(report)
}

#[test]
#[ignore = "run only through source/binary/input-capturing private qualification harness"]
fn selected_fixture() {
    let path = env::var_os("SPELLWAKE_PRIVATE_INPUT").expect("explicit input path required");
    let case = env::var("SPELLWAKE_PRIVATE_CASE").expect("explicit selected case required");
    let output = env::var_os("SPELLWAKE_PRIVATE_OUTPUT").expect("explicit output path required");
    let result = produce(Path::new(&path), &case);
    let report = match &result {
        Ok(value) => value.clone(),
        Err(error) => {
            json!({"schema_version":crate::test_support::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","profile":"ordinary-trigger-private-unit","case":case,"status":"failed","failure":error})
        }
    };
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap())
        .expect("write actual private observations");
    assert!(result.is_ok(), "private producer failed: {result:?}");
    assert_eq!(report["producer_integrity"], true);
    // Successful capture preserves owner errors; the independent checker adjudicates them.
}

#[test]
#[ignore = "run only through source/binary/input-capturing private qualification v2 harness"]
fn selected_fixture_v2() {
    v2::selected_fixture();
}
