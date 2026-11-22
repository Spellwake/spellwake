//! Saved-snapshot program-binding coverage CLI. Does not qualify whole cards.
use spellwake_card_programs::{
    CURRENT_KEYWORD_PROFILE, KEYWORD_CREATURE_PROFILE, KEYWORD_GAMEPLAY_PROFILE,
    KEYWORD_GAMEPLAY_V3_PROFILE, KEYWORD_GAMEPLAY_V4_PROFILE, KEYWORD_GAMEPLAY_V5_PROFILE,
    KEYWORD_GAMEPLAY_V6_PROFILE, OPTIONAL_PAYMENT_PROFILE, Registry, RegistryError,
};
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
    let profile = match args.as_slice() {
        [_, _, flag, _] if flag == "--creature-vocabulary-input" => None,
        [_, _, flag, _, selector, profile]
            if flag == "--creature-vocabulary-input" && selector == "--profile"
                && matches!(profile.as_str(), OPTIONAL_PAYMENT_PROFILE | KEYWORD_CREATURE_PROFILE | KEYWORD_GAMEPLAY_PROFILE | KEYWORD_GAMEPLAY_V3_PROFILE | KEYWORD_GAMEPLAY_V4_PROFILE | KEYWORD_GAMEPLAY_V5_PROFILE | KEYWORD_GAMEPLAY_V6_PROFILE | CURRENT_KEYWORD_PROFILE) => Some(profile.as_str()),
        _ => return Err(Failure::Snapshot(SnapshotError::Invalid {
            field: "arguments",
            message: "expected metadata JSON and compressed Oracle JSONL paths followed by --creature-vocabulary-input PATH and an optional supported --profile NAME".into(),
        })),
    };
    let inventory = load(Path::new(&args[0]), Path::new(&args[1])).map_err(Failure::Snapshot)?;
    let vocabulary =
        creature_vocabulary::load_vocabulary(Path::new(&args[3])).map_err(|message| {
            Failure::Snapshot(SnapshotError::Invalid {
                field: "creature-vocabulary-input",
                message,
            })
        })?;
    match profile {
        Some(KEYWORD_GAMEPLAY_V6_PROFILE) => {
            Registry::from_sixth_keyword_inventory(&inventory, &vocabulary)
        }
        Some(KEYWORD_GAMEPLAY_V5_PROFILE) => {
            Registry::from_fifth_keyword_inventory(&inventory, &vocabulary)
        }
        Some(KEYWORD_GAMEPLAY_V4_PROFILE) => {
            Registry::from_fourth_keyword_inventory(&inventory, &vocabulary)
        }
        Some(CURRENT_KEYWORD_PROFILE) => {
            Registry::from_current_keyword_inventory(&inventory, &vocabulary)
        }
        Some(KEYWORD_GAMEPLAY_V3_PROFILE) => {
            Registry::from_extended_keyword_inventory(&inventory, &vocabulary)
        }
        Some(KEYWORD_GAMEPLAY_PROFILE) => {
            Registry::from_keyword_gameplay_inventory(&inventory, &vocabulary)
        }
        Some(KEYWORD_CREATURE_PROFILE) => {
            Registry::from_keyword_creature_inventory(&inventory, &vocabulary)
        }
        Some(_) => Registry::from_optional_payment_inventory(&inventory, &vocabulary),
        None => Registry::from_inventory(&inventory, &vocabulary),
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
