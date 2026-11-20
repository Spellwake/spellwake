//! Local engineering entry point for a source-bound release and deck review.
use spellwake_application::native_game::inspect_release_candidate;
use std::{io, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let paths: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if let [
        command,
        metadata,
        snapshot,
        vocabulary,
        room,
        basis,
        release,
        legality,
    ] = paths.as_slice()
        && command == std::path::Path::new("room-candidate")
    {
        let result = (|| -> Result<String, Box<dyn std::error::Error>> {
            fn value(path: &std::path::Path) -> Result<&str, &'static str> {
                path.to_str().ok_or("invalid argument")
            }
            spellwake_application::native_game::inspect_constructed_room(
                metadata,
                snapshot,
                vocabulary,
                value(room)?.parse()?,
                &value(basis)?.parse()?,
                value(release)?.parse()?,
                value(legality)?.parse()?,
            )
        })();
        return match result {
            Ok(reference) => {
                println!("{reference}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    let [metadata, snapshot, vocabulary, rules, deck] = paths.as_slice() else {
        eprintln!("usage: native_release_candidate METADATA SNAPSHOT VOCABULARY RULES DECK");
        return ExitCode::FAILURE;
    };
    match inspect_release_candidate(metadata, snapshot, vocabulary, rules, deck) {
        Ok(report) => {
            if serde_json::to_writer_pretty(io::stdout().lock(), &report).is_err()
                || report["deckConstruction"].get("Err").is_some()
            {
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
