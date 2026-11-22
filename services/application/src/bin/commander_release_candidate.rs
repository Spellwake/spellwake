//! Emit release-review source metadata from the guarded Commander registry.
use spellwake_application::native_game::{NativeProgramPool, NativeProgramSource};
use std::{error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [metadata, snapshot, vocabulary] = args.as_slice() else {
        return Err("usage: commander_release_candidate METADATA CATALOGUE VOCABULARY".into());
    };
    let source = NativeProgramSource::from_bytes(
        NativeProgramPool::KeywordGameplay,
        &fs::read(Path::new(metadata))?,
        fs::read(Path::new(snapshot))?,
        &fs::read(Path::new(vocabulary))?,
    )?;
    println!("{}", source.commander_pilot_source()?);
    Ok(())
}
