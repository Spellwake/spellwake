//! Local saved-snapshot inventory CLI. No network, catalogue publication or card execution.
use spellwake_content::snapshot::{SnapshotError, load};
use spellwake_contracts::ApiResponse;
use std::{path::Path, process::ExitCode};
fn run() -> Result<spellwake_content::Inventory, SnapshotError> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(SnapshotError::Invalid {
            field: "arguments",
            message: "expected metadata JSON and compressed Oracle JSONL paths".into(),
        });
    }
    load(Path::new(&args[0]), Path::new(&args[1]))
}
fn main() -> ExitCode {
    let result = run();
    let success = result.is_ok();
    let output: ApiResponse<_> = match result {
        Ok(report) => ApiResponse::Success { data: report },
        Err(SnapshotError::Invalid { field, message }) => {
            eprintln!("invalid catalogue input: {message}");
            ApiResponse::Fail {
                data: std::collections::BTreeMap::from([(
                    field.to_owned(),
                    "invalid-input".into(),
                )]),
            }
        }
        Err(SnapshotError::Operational(error)) => {
            eprintln!("catalogue inventory failed: {error}");
            ApiResponse::Error {
                message: "Catalogue inventory unavailable; inspect local diagnostics".into(),
                code: "catalogue-inventory-unavailable".into(),
            }
        }
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout().lock(), &output) {
        eprintln!("inventory output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
