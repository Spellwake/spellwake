//! One current-schema input issuer and saved typed-input replay for selected stages.
pub mod input;
mod policy;
use crate::{program, projection};
mod capture;
mod runner;
#[cfg(test)]
mod tests;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
pub fn run_file(path: &Path, case: &str, policy: input::StagePolicy) -> Value {
    let bytes = match fs::read(path) {
        Ok(value) => value,
        Err(error) => {
            return json!({"schema_version":crate::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","profile":policy.profile(),"case":case,"status":"failed","failure":format!("input read failed: {error}")});
        }
    };
    let before = format!("{:x}", Sha256::digest(&bytes));
    let mut report = if before != policy.input_sha256() {
        json!({"status":"failed","failure":"input bytes do not match the selected current-schema stage snapshot"})
    } else {
        match input::parse(&bytes, case, policy) {
            Ok(selected) => {
                let sources = [
                    path.to_path_buf(),
                    crate::current_capture::repository()
                        .join("crates/reference_tools/src/root_recipes/mod.rs"),
                    crate::current_capture::repository()
                        .join("crates/reference_tools/src/root_recipes/capture.rs"),
                    crate::current_capture::repository()
                        .join("crates/reference_tools/src/root_recipes/input.rs"),
                    crate::current_capture::repository()
                        .join("crates/reference_tools/src/root_recipes/runner.rs"),
                    crate::current_capture::repository()
                        .join("crates/reference_tools/src/root_recipes/policy.rs"),
                ];
                let mut report = match crate::current_capture::recorder(
                    &selected.vocabulary,
                    &sources,
                    crate::current_profile::RegistryWitness::NotApplicable,
                ) {
                    Ok(recorder) => runner::run_with_recorder(&selected, recorder),
                    Err(error) => {
                        json!({"status":"failed","failure":error,"initialization":"not attempted","records":[]})
                    }
                };
                report["selected_input"] = selected.raw_fixture.clone();
                report["selected_definitions"] = json!(selected.definitions);
                report["packet_metadata"] = selected.packet_metadata;
                report["recipe_metadata"] = json!({"id":selected.fixture.id,"scenario":selected.fixture.scenario,"forge_route_descriptor_only":selected.fixture.forge_route,"notes":selected.fixture.notes,"card_id_base":selected.fixture.card_id_base,"native_binding_contract":selected.fixture.engine_recipes.native.bindings,"budgets":{"max_root_inputs":selected.fixture.engine_recipes.native.budget.max_root_inputs,"max_native_auto_work_returns":selected.fixture.engine_recipes.native.budget.max_native_auto_work_returns,"max_private_owner_calls":selected.fixture.engine_recipes.native.budget.max_private_owner_calls},"external_native_auto_work_calls":0,"budget_scope":"root initialize/advance own internal work; no external auto-work or private-owner calls"});
                report
            }
            Err(error) => {
                json!({"status":if error.starts_with("unsupported_scope:"){"unsupported_scope"}else{"failed"},"failure":error})
            }
        }
    };
    let after = fs::read(path)
        .map(|value| format!("{:x}", Sha256::digest(value)))
        .map_err(|error| error.to_string());
    let unchanged = after.as_ref().is_ok_and(|value| *value == before);
    report["schema_version"] = json!(crate::current_profile::REPORT_SCHEMA_VERSION);
    report["record_codec"] = json!("spellwake.current-native-qualification.v2");
    report["profile"] = json!(policy.profile());
    report["case"] = json!(case);
    report["input_source"] = json!({"path":path,"sha256_before":before,"sha256_after":after.as_ref().ok(),"sha256_after_error":after.as_ref().err(),"bytes":bytes.len(),"unchanged":unchanged});
    report["participant_codec"] = json!({"A":0,"B":1,"C":2,"D":3,"E":4,"F":5});
    if !unchanged {
        report["status"] = json!("failed");
        report["input_changed_failure"] =
            json!("input bytes changed or became unreadable during producer execution");
    }
    report
}

#[cfg(test)]
pub(crate) fn contract_lower_reply(
    state: &spellwake_engine::game::GameState,
    items: &[serde_json::Value],
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<spellwake_engine::game::DecisionReply, String> {
    runner::lower_creature_type_reply(state, items, vocabulary)
}
