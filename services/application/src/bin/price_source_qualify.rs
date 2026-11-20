//! Read-only source qualification: downloads local evidence without publishing a sample.
use spellwake_application::{ApiResponse, price_source::qualify_current};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    let [directory] = arguments.as_slice() else {
        eprintln!("usage: price_source_qualify <local-evidence-directory>");
        return ExitCode::FAILURE;
    };
    eprintln!("price_source phase=acquiring_and_validating");
    match qualify_current(&PathBuf::from(directory)) {
        Ok(evidence) => {
            match serde_json::to_writer(
                std::io::stdout().lock(),
                &ApiResponse::Success { data: evidence },
            ) {
                Ok(()) => {
                    println!();
                    ExitCode::SUCCESS
                }
                Err(_) => {
                    eprintln!("price_source code=output_failed");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("price_source code={error:?}");
            let response: ApiResponse<()> = ApiResponse::Error {
                message: error.to_string(),
                code: "price_source_qualification_failed".to_owned(),
            };
            if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
                eprintln!("price_source code=output_failed");
            }
            println!();
            ExitCode::FAILURE
        }
    }
}
