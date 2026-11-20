//! Explicit local saved-snapshot inventory and source-qualified creature vocabulary CLI.
use spellwake_content::creature_types::{
    CreatureTypeCompilation, CreatureTypeCompileError, CreatureTypeSource, compile, read_source,
    write_output,
};
use spellwake_content::snapshot::{SnapshotError, load};
use spellwake_contracts::{ApiResponse, creature_types::CreatureVocabularyId};
use std::{path::Path, process::ExitCode};
fn inventory(args: &[String]) -> Result<spellwake_content::Inventory, SnapshotError> {
    if args.len() != 2 {
        return Err(SnapshotError::Invalid {
            field: "arguments",
            message: "expected metadata JSON and compressed Oracle JSONL paths".into(),
        });
    }
    load(Path::new(&args[0]), Path::new(&args[1]))
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|argument| argument == "creature-vocabulary")
    {
        return creature_vocabulary(&args[1..]);
    }
    let result = inventory(&args);
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
