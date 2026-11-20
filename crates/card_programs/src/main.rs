//! Saved-snapshot program-binding coverage CLI. Does not qualify whole cards.
use spellwake_card_programs::{OPTIONAL_PAYMENT_PROFILE, Registry, RegistryError};
#[path = "../../engine/test_support/creature_vocabulary.rs"]
pub mod creature_vocabulary;
use spellwake_content::snapshot::{SnapshotError, load};
use spellwake_contracts::ApiResponse;
use std::{collections::BTreeMap, path::Path, process::ExitCode};

enum Failure {
    Snapshot(SnapshotError),
    Registry(RegistryError),
}
fn run() -> Result<Registry, Failure> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let optional_payment = args.len() == 6
        && args[2] == "--creature-vocabulary-input"
        && args[4] == "--profile"
        && args[5] == OPTIONAL_PAYMENT_PROFILE;
    if !optional_payment && (args.len() != 4 || args[2] != "--creature-vocabulary-input") {
        return Err(Failure::Snapshot(SnapshotError::Invalid {
            field: "arguments",
            message: "expected metadata JSON and compressed Oracle JSONL paths followed by --creature-vocabulary-input PATH".into(),
        }));
    }
    let inventory = load(Path::new(&args[0]), Path::new(&args[1])).map_err(Failure::Snapshot)?;
    let vocabulary =
        creature_vocabulary::load_vocabulary(Path::new(&args[3])).map_err(|message| {
            Failure::Snapshot(SnapshotError::Invalid {
                field: "creature-vocabulary-input",
                message,
            })
        })?;
    if optional_payment {
        Registry::from_optional_payment_inventory(&inventory, &vocabulary)
    } else {
        Registry::from_inventory(&inventory, &vocabulary)
    }
    .map_err(Failure::Registry)
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
