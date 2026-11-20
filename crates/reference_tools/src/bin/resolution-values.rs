//! Exact selected public root recipes for resolution-local value qualification.
use spellwake_reference_tools::{creature_types, resolution_values};
use std::{path::Path, process::ExitCode};
fn main() -> ExitCode {
    let args = match creature_types::qualification_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            return ExitCode::FAILURE;
        }
    };
    let report = match args.as_slice() {
        [input, path, case_flag, case] if input == "--inputs" && case_flag == "--case" => {
            resolution_values::run_file(Path::new(path), case)
        }
        [matched, input, path, case_flag, case]
            if matched == "--matched-reference" && input == "--inputs" && case_flag == "--case" =>
        {
            resolution_values::run_matched_file(Path::new(path), case)
        }
        _ => {
            serde_json::json!({"schema_version":spellwake_reference_tools::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","status":"failed","failure":"usage: resolution-values [--matched-reference] --inputs PATH --case ID --creature-vocabulary-input PATH"})
        }
    };
    match serde_json::to_writer(std::io::stdout(), &report) {
        Ok(()) if report["status"] == "completed_recipe" => ExitCode::SUCCESS,
        Ok(()) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("resolution-values output failed: {error}");
            ExitCode::FAILURE
        }
    }
}
