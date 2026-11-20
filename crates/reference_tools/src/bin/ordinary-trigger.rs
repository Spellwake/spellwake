//! Explicit selected-input public-root producer. Outputs actual records even on failure.
use spellwake_reference_tools::ordinary_trigger::run_file;
use std::{path::Path, process::ExitCode};
fn main() -> ExitCode {
    let args = match spellwake_reference_tools::creature_types::qualification_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            return ExitCode::FAILURE;
        }
    };
    let report = match args.as_slice() {
        [input_flag, path, case_flag, case]
            if input_flag == "--inputs" && case_flag == "--case" =>
        {
            run_file(Path::new(path), case)
        }
        _ => {
            serde_json::json!({"schema_version":spellwake_reference_tools::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","status":"failed","failure":"usage: ordinary-trigger --inputs PATH --case ID --creature-vocabulary-input PATH"})
        }
    };
    match serde_json::to_string(&report) {
        Ok(line) => {
            println!("{line}");
            if report["status"] == "completed_recipe" {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("ordinary-trigger serialization failed: {error}");
            ExitCode::FAILURE
        }
    }
}
