//! Saved-snapshot program-binding coverage CLI. Does not qualify whole cards.
use spellwake_card_programs::{Registry, RegistryError};
use spellwake_content::snapshot::{SnapshotError, load};
use spellwake_contracts::ApiResponse;
use std::{collections::BTreeMap, path::Path, process::ExitCode};

enum Failure {
    Snapshot(SnapshotError),
    Registry(RegistryError),
}
fn run() -> Result<Registry, Failure> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err(Failure::Snapshot(SnapshotError::Invalid {
            field: "arguments",
            message: "expected metadata JSON and compressed Oracle JSONL paths".into(),
        }));
    }
    let inventory = load(Path::new(&args[0]), Path::new(&args[1])).map_err(Failure::Snapshot)?;
    Registry::from_inventory(&inventory).map_err(Failure::Registry)
}
fn main() -> ExitCode {
    let registry = match run() {
        Ok(registry) => registry,
        Err(error) => {
            let response: ApiResponse<()> = match error {
                Failure::Snapshot(SnapshotError::Invalid { field, message }) => {
                    eprintln!("invalid registry source: {message}");
                    ApiResponse::Fail {
                        data: BTreeMap::from([(field.into(), "invalid-input".into())]),
                    }
                }
                Failure::Snapshot(SnapshotError::Operational(error)) => {
                    eprintln!("registry source failed: {error}");
                    ApiResponse::Error {
                        message: "Registry source unavailable".into(),
                        code: "registry-source-unavailable".into(),
                    }
                }
                Failure::Registry(error) => {
                    eprintln!("registry construction failed: {error}");
                    ApiResponse::Error {
                        message: "Registry construction unavailable".into(),
                        code: "registry-unavailable".into(),
                    }
                }
            };
            if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
                eprintln!("registry output failed: {error}");
            }
            return ExitCode::FAILURE;
        }
    };
    let response: ApiResponse<_> = ApiResponse::Success {
        data: registry.coverage(),
    };
    if let Err(error) = serde_json::to_writer(std::io::stdout(), &response) {
        eprintln!("registry output failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
