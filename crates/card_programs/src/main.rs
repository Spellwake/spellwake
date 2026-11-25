//! Saved-snapshot program-binding coverage and incremental catalog-support CLI.
//! Does not claim whole-card gameplay qualification.
use spellwake_card_programs::{
    CURRENT_KEYWORD_PROFILE, KEYWORD_CREATURE_PROFILE, KEYWORD_GAMEPLAY_PROFILE,
    KEYWORD_GAMEPLAY_V3_PROFILE, KEYWORD_GAMEPLAY_V4_PROFILE, KEYWORD_GAMEPLAY_V5_PROFILE,
    KEYWORD_GAMEPLAY_V6_PROFILE, OPTIONAL_PAYMENT_PROFILE, Registry, RegistryError,
    catalog_support::{CatalogSupportCache, evaluate_catalog_support},
};
#[path = "../../engine/test_support/creature_vocabulary.rs"]
pub mod creature_vocabulary;
use spellwake_content::{
    refresh::CatalogRefreshState,
    snapshot::{SnapshotError, load},
};
use spellwake_contracts::ApiResponse;
use std::{collections::BTreeMap, path::Path, process::ExitCode};

enum Failure {
    Snapshot(SnapshotError),
    Registry(RegistryError),
}
fn run(args: &[String]) -> Result<Registry, Failure> {
    let profile = match args {
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

fn emit_json<T: serde::Serialize>(response: &ApiResponse<T>, success: bool) -> ExitCode {
    if let Err(error) = serde_json::to_writer(std::io::stdout().lock(), response) {
        eprintln!("registry output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fail_json(field: &str, code: &str) -> ExitCode {
    let response: ApiResponse<()> = ApiResponse::Fail {
        data: BTreeMap::from([(field.to_owned(), code.to_owned())]),
    };
    emit_json(&response, false)
}

fn error_json(message: &str, code: &str) -> ExitCode {
    let response: ApiResponse<()> = ApiResponse::Error {
        message: message.to_owned(),
        code: code.to_owned(),
    };
    emit_json(&response, false)
}

fn write_atomic_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

fn run_support_report(args: &[String]) -> ExitCode {
    if args.len() < 2 {
        eprintln!(
            "usage: spellwake-card-programs support-report <metadata.json> <snapshot.jsonl.gz> --creature-vocabulary-input <vocab.json> [--previous-state <state.json>] [--write-state <state.json>] [--cache <cache.json>] [--write-cache <cache.json>]"
        );
        return fail_json("arguments", "invalid-input");
    }
    let metadata_path = Path::new(&args[0]);
    let snapshot_path = Path::new(&args[1]);
    let mut vocab_path: Option<&Path> = None;
    let mut previous_state_path: Option<&Path> = None;
    let mut write_state_path: Option<&Path> = None;
    let mut cache_path: Option<&Path> = None;
    let mut write_cache_path: Option<&Path> = None;
    let mut idx = 2;
    while idx < args.len() {
        let Some(value) = args.get(idx + 1) else {
            eprintln!("missing value for option {}", args[idx]);
            return fail_json("arguments", "invalid-input");
        };
        match args[idx].as_str() {
            "--creature-vocabulary-input" => vocab_path = Some(Path::new(value)),
            "--previous-state" => previous_state_path = Some(Path::new(value)),
            "--write-state" => write_state_path = Some(Path::new(value)),
            "--cache" => cache_path = Some(Path::new(value)),
            "--write-cache" => write_cache_path = Some(Path::new(value)),
            other => {
                eprintln!("unknown support-report option: {other}");
                return fail_json("arguments", "invalid-input");
            }
        }
        idx += 2;
    }
    let Some(vocab_path) = vocab_path else {
        eprintln!("missing required --creature-vocabulary-input");
        return fail_json("creature-vocabulary-input", "invalid-input");
    };

    let inventory = match load(metadata_path, snapshot_path) {
        Ok(inv) => inv,
        Err(SnapshotError::Invalid { field, message }) => {
            eprintln!("invalid catalogue source: {message}");
            return fail_json(field, "invalid-input");
        }
        Err(SnapshotError::Operational(error)) => {
            eprintln!("catalogue source unavailable: {error}");
            return error_json(
                "Catalogue source unavailable",
                "catalogue-source-unavailable",
            );
        }
    };
    let vocabulary = match creature_vocabulary::load_vocabulary(vocab_path) {
        Ok(vocab) => vocab,
        Err(message) => {
            eprintln!("invalid creature vocabulary: {message}");
            return fail_json("creature-vocabulary-input", "invalid-input");
        }
    };
    let previous_state: Option<CatalogRefreshState> = match previous_state_path {
        None => None,
        Some(path) => match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(state) => Some(state),
                Err(error) => {
                    eprintln!("invalid previous refresh state: {error}");
                    return fail_json("previousState", "invalid-input");
                }
            },
            Err(error) => {
                eprintln!("previous refresh state unreadable: {error}");
                return error_json(
                    "Previous refresh state unavailable",
                    "previous-state-unavailable",
                );
            }
        },
    };
    let previous_cache: Option<CatalogSupportCache> = match cache_path {
        None => None,
        Some(path) => match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(cache) => Some(cache),
                Err(error) => {
                    eprintln!("ignoring unreadable/incompatible support cache: {error}");
                    None
                }
            },
            Err(error) => {
                eprintln!("support cache unreadable: {error}");
                return error_json("Support cache unavailable", "support-cache-unavailable");
            }
        },
    };

    let (report, next_state, next_cache) = match evaluate_catalog_support(
        &inventory,
        &vocabulary,
        previous_state.as_ref(),
        previous_cache.as_ref(),
    ) {
        Ok(outputs) => outputs,
        Err(error) => {
            eprintln!("catalog support evaluation failed: {error}");
            return error_json(
                "Catalog support evaluation failed",
                "catalog-support-evaluation-failed",
            );
        }
    };

    if let Some(path) = write_state_path
        && let Err(error) = write_atomic_json(path, &next_state)
    {
        eprintln!("failed to write refresh state: {error}");
        return error_json("Failed to persist refresh state", "state-write-failed");
    }
    if let Some(path) = write_cache_path
        && let Err(error) = write_atomic_json(path, &next_cache)
    {
        eprintln!("failed to write support cache: {error}");
        return error_json("Failed to persist support cache", "cache-write-failed");
    }

    emit_json(&ApiResponse::Success { data: report }, true)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|arg| arg == "support-report") {
        return run_support_report(&args[1..]);
    }
    let registry = match run(&args) {
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
            return emit_json(&response, false);
        }
    };
    let response: ApiResponse<_> = ApiResponse::Success {
        data: registry.coverage(),
    };
    emit_json(&response, true)
}
