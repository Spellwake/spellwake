//! Explicit operator entry point; credentials stay in runtime configuration.
use spellwake_application::{ApiResponse, catalogue_publisher::CataloguePublisher};
use std::{path::PathBuf, process::ExitCode};
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [qualification, destination] = args.as_slice() else {
        eprintln!("usage: catalogue_publish <qualification.json> <existing-stage-directory>");
        return ExitCode::FAILURE;
    };
    let result = CataloguePublisher::from_env().and_then(|publisher| {
        publisher.publish(&PathBuf::from(qualification), &PathBuf::from(destination))
    });
    match result {
        Ok(report) => {
            if serde_json::to_writer(
                std::io::stdout().lock(),
                &ApiResponse::Success { data: report },
            )
            .is_err()
            {
                eprintln!("catalogue_publisher code=output_failed");
                return ExitCode::FAILURE;
            }
            println!();
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("catalogue_publisher code={:?}", error.code());
            let response: ApiResponse<()> = ApiResponse::Error {
                message: error.to_string(),
                code: "catalogue_publication_unresolved".to_owned(),
            };
            if serde_json::to_writer(std::io::stdout().lock(), &response).is_err() {
                eprintln!("catalogue_publisher code=output_failed");
            }
            println!();
            ExitCode::FAILURE
        }
    }
}
