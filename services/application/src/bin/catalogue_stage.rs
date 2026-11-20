//! Reverify original retained evidence and produce a source-bound local metadata stage.
use spellwake_application::{ApiResponse, catalogue_source::stage_catalogue};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [qualification, destination] = args.as_slice() else {
        eprintln!("usage: catalogue_stage <qualification.json> <existing-stage-directory>");
        return ExitCode::FAILURE;
    };
    eprintln!("catalogue_source phase=verifying_and_staging publication=none");
    match stage_catalogue(&PathBuf::from(qualification), &PathBuf::from(destination)) {
        Ok((marker, stage)) => {
            let response = ApiResponse::Success {
                data: serde_json::json!({ "marker": marker, "stage": stage }),
            };
            if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
                eprintln!("catalogue_source code=output_failed");
                return ExitCode::FAILURE;
            }
            println!();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("catalogue_source code={error:?}");
            let response: ApiResponse<()> = ApiResponse::Error {
                message: "Catalogue source verification failed.".to_owned(),
                code: "catalogue_source_failed".to_owned(),
            };
            if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
                eprintln!("catalogue_source code=output_failed");
            }
            println!();
            ExitCode::FAILURE
        }
    }
}
