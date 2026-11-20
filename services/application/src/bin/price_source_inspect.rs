//! Reverify retained evidence without inventing a fresh acquisition timestamp.
use spellwake_application::{ApiResponse, price_source::inspect_retained};
use std::{path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [artifact, index] = args.as_slice() else {
        eprintln!("usage: price_source_inspect <artifact.jsonl.gz> <bulk-index.json>");
        return ExitCode::FAILURE;
    };
    match inspect_retained(&PathBuf::from(artifact), &PathBuf::from(index)) {
        Ok(evidence) => {
            if serde_json::to_writer(
                std::io::stdout().lock(),
                &ApiResponse::Success { data: evidence },
            )
            .is_err()
            {
                eprintln!("price_source code=output_failed");
                return ExitCode::FAILURE;
            }
            println!();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("price_source code={error:?}");
            let response: ApiResponse<()> = ApiResponse::Error {
                message: error.to_string(),
                code: "price_source_inspection_failed".into(),
            };
            if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
                eprintln!("price_source code=output_failed");
            }
            println!();
            ExitCode::FAILURE
        }
    }
}
