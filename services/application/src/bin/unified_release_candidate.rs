//! Report exact Constructed and Limited program inputs from the unified profile.
use spellwake_application::native_game::{NativeProgramPool, inspect_release_candidate};
use std::{io, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [metadata, snapshot, vocabulary, rules, deck] = paths.as_slice() else {
        eprintln!("usage: unified_release_candidate METADATA SNAPSHOT VOCABULARY RULES DECK");
        return ExitCode::FAILURE;
    };
    match inspect_release_candidate(
        NativeProgramPool::KeywordGameplay,
        metadata,
        snapshot,
        vocabulary,
        rules,
        deck,
    ) {
        Ok(report) => {
            if serde_json::to_writer_pretty(io::stdout().lock(), &report).is_err()
                || report["deckConstruction"].get("Err").is_some()
            {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
