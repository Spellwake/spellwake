//! Explicit local saved-snapshot inventory, isolated preview-format inventory,
//! incremental catalog diffing, and source-qualified creature vocabulary CLI.
use spellwake_content::creature_types::{
    CreatureTypeCompilation, CreatureTypeCompileError, CreatureTypeSource, compile, read_source,
    write_output,
};
use spellwake_content::refresh::{CatalogRefreshState, build_refresh_state, diff_refresh_states};
use spellwake_content::snapshot::{SnapshotError, load};
use spellwake_contracts::{ApiResponse, creature_types::CreatureVocabularyId};
use std::{collections::BTreeMap, path::Path, process::ExitCode};

fn inventory(args: &[String]) -> Result<spellwake_content::Inventory, SnapshotError> {
    if args.len() != 2 {
        return Err(SnapshotError::Invalid {
            field: "arguments",
            message: "expected metadata JSON and compressed Oracle JSONL paths".into(),
        });
    }
    load(Path::new(&args[0]), Path::new(&args[1]))
}

fn emit_json<T: serde::Serialize>(output: &ApiResponse<T>, success: bool) -> ExitCode {
    if let Err(error) = serde_json::to_writer(std::io::stdout().lock(), output) {
        eprintln!("catalogue output failed: {error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fail_response<T>(field: &str, code: &str) -> ApiResponse<T> {
    ApiResponse::Fail {
        data: BTreeMap::from([(field.to_owned(), code.to_owned())]),
    }
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("creature-vocabulary") => return creature_vocabulary(&args[1..]),
        Some("preview-inventory") => return preview_inventory(&args[1..]),
        Some("incremental-diff") => return incremental_diff(&args[1..]),
        _ => {}
    }
    let result = inventory(&args);
    let success = result.is_ok();
    let output: ApiResponse<_> = match result {
        Ok(report) => ApiResponse::Success { data: report },
        Err(SnapshotError::Invalid { field, message }) => {
            eprintln!("invalid catalogue input: {message}");
            fail_response(field, "invalid-input")
        }
        Err(SnapshotError::Operational(error)) => {
            eprintln!("catalogue inventory failed: {error}");
            ApiResponse::Error {
                message: "Catalogue inventory unavailable; inspect local diagnostics".into(),
                code: "catalogue-inventory-unavailable".into(),
            }
        }
    };
    emit_json(&output, success)
}

fn preview_inventory(args: &[String]) -> ExitCode {
    let result = inventory(args);
    let success = result.is_ok();
    let output: ApiResponse<_> = match result {
        Ok(report) => ApiResponse::Success {
            data: report.preview_inventory(),
        },
        Err(SnapshotError::Invalid { field, message }) => {
            eprintln!("invalid preview catalogue input: {message}");
            fail_response(field, "invalid-input")
        }
        Err(SnapshotError::Operational(error)) => {
            eprintln!("preview catalogue inventory failed: {error}");
            ApiResponse::Error {
                message: "Preview catalogue inventory unavailable; inspect local diagnostics"
                    .into(),
                code: "preview-catalogue-unavailable".into(),
            }
        }
    };
    emit_json(&output, success)
}

fn incremental_diff(args: &[String]) -> ExitCode {
    if args.len() < 2 {
        eprintln!(
            "invalid incremental-diff arguments: expected metadata JSON and compressed Oracle JSONL paths"
        );
        return emit_json::<()>(&fail_response("arguments", "invalid-input"), false);
    }
    let metadata_path = Path::new(&args[0]);
    let snapshot_path = Path::new(&args[1]);
    let mut previous_state_path: Option<&Path> = None;
    let mut write_state_path: Option<&Path> = None;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--previous-state" => {
                let Some(path) = args.get(index + 1) else {
                    eprintln!("missing path after --previous-state");
                    return emit_json::<()>(&fail_response("arguments", "invalid-input"), false);
                };
                previous_state_path = Some(Path::new(path));
                index += 2;
            }
            "--write-state" => {
                let Some(path) = args.get(index + 1) else {
                    eprintln!("missing path after --write-state");
                    return emit_json::<()>(&fail_response("arguments", "invalid-input"), false);
                };
                write_state_path = Some(Path::new(path));
                index += 2;
            }
            other => {
                eprintln!("unexpected incremental-diff argument: {other}");
                return emit_json::<()>(&fail_response("arguments", "invalid-input"), false);
            }
        }
    }
    let previous_state: Option<CatalogRefreshState> = match previous_state_path {
        None => None,
        Some(path) => match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(state) => Some(state),
                Err(error) => {
                    eprintln!("invalid previous refresh state: {error}");
                    return emit_json::<()>(
                        &fail_response("previousState", "invalid-input"),
                        false,
                    );
                }
            },
            Err(error) => {
                eprintln!("failed to read previous refresh state: {error}");
                return emit_json::<()>(
                    &ApiResponse::Error {
                        message: "Previous refresh state unavailable; inspect local diagnostics"
                            .into(),
                        code: "previous-state-unavailable".into(),
                    },
                    false,
                );
            }
        },
    };
    let report = match load(metadata_path, snapshot_path) {
        Ok(report) => report,
        Err(SnapshotError::Invalid { field, message }) => {
            eprintln!("invalid catalogue input: {message}");
            return emit_json::<()>(&fail_response(field, "invalid-input"), false);
        }
        Err(SnapshotError::Operational(error)) => {
            eprintln!("catalogue inventory failed: {error}");
            return emit_json::<()>(
                &ApiResponse::Error {
                    message: "Catalogue inventory unavailable; inspect local diagnostics".into(),
                    code: "catalogue-inventory-unavailable".into(),
                },
                false,
            );
        }
    };
    let current_state = match build_refresh_state(&report) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("refresh state build failed: {error}");
            return emit_json::<()>(&fail_response("snapshot", "invalid-input"), false);
        }
    };
    let diff = diff_refresh_states(previous_state.as_ref(), &current_state);
    if let Some(out_path) = write_state_path {
        let encoded = match serde_json::to_vec_pretty(&current_state) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("refresh state encode failed: {error}");
                return emit_json::<()>(
                    &ApiResponse::Error {
                        message: "Refresh state serialization failed".into(),
                        code: "refresh-state-encode-failed".into(),
                    },
                    false,
                );
            }
        };
        if let Err(error) = std::fs::write(out_path, encoded) {
            eprintln!("failed to write refresh state: {error}");
            return emit_json::<()>(
                &ApiResponse::Error {
                    message: "Failed to persist refresh state; inspect local diagnostics".into(),
                    code: "refresh-state-write-failed".into(),
                },
                false,
            );
        }
    }
    emit_json(&ApiResponse::Success { data: diff }, true)
}

fn creature_vocabulary(args: &[String]) -> ExitCode {
    let result: Result<CreatureVocabularyId, _> = if args.len() == 3 {
        args[2].clone().try_into()
    } else {
        String::new().try_into()
    };
    let digest = match result {
        Ok(digest)
            if !args[1].is_empty()
                && args[1].trim() == args[1]
                && !args[1].chars().any(char::is_control) =>
        {
            digest
        }
        _ => {
            eprintln!(
                "invalid creature vocabulary arguments: expected source path, rules revision and canonical lowercase SHA-256"
            );
            return creature_output(
                ApiResponse::Fail {
                    data: std::collections::BTreeMap::from([(
                        "arguments".into(),
                        "invalid-input".into(),
                    )]),
                },
                false,
            );
        }
    };
    let bytes = match read_source(Path::new(&args[0])) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{error}");
            return creature_output(
                ApiResponse::Error {
                    message: "Creature-type source unavailable; inspect local diagnostics".into(),
                    code: "creature-type-source-read-failed".into(),
                },
                false,
            );
        }
    };
    match compile(
        &bytes,
        CreatureTypeSource {
            rules_revision: args[1].clone(),
            rules_sha256: *digest.as_bytes(),
        },
    ) {
        Ok(compilation) => creature_output(ApiResponse::Success { data: compilation }, true),
        Err(error) => {
            eprintln!("creature-type compilation failed: {error}");
            let code = match error {
                CreatureTypeCompileError::SourceDigestMismatch { .. } => "source-digest-mismatch",
                CreatureTypeCompileError::InvalidUtf8 { .. } => "invalid-source-encoding",
                CreatureTypeCompileError::Vocabulary(_) => "invalid-vocabulary",
            };
            creature_output(
                ApiResponse::Fail {
                    data: std::collections::BTreeMap::from([("source".into(), code.into())]),
                },
                false,
            )
        }
    }
}

fn creature_output(output: ApiResponse<CreatureTypeCompilation>, success: bool) -> ExitCode {
    if let Err(error) = write_output(std::io::stdout().lock(), &output) {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
