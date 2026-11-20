//! Actual saved-catalogue teaching games over the shared ordinary runner.
use spellwake_contracts::ApiResponse;
use spellwake_reference_tools::{
    creature_types, current_capture,
    ordinary_game::teaching::{TeachingCase, TeachingSource},
};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};
fn run() -> Result<(), String> {
    let vocabulary = creature_types::selected_vocabulary()?;
    let args = creature_types::qualification_args()?;
    let mut metadata = None;
    let mut snapshot = None;
    let mut cases = TeachingCase::ALL.to_vec();
    let mut capture = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--metadata" | "--snapshot" | "--case" => {
                let value = args.get(index + 1).ok_or("missing teaching argument value")?;
                match args[index].as_str() {
                    "--metadata" => metadata = Some(PathBuf::from(value)),
                    "--snapshot" => snapshot = Some(PathBuf::from(value)),
                    "--case" => cases = vec![TeachingCase::parse(value)?],
                    _ => unreachable!("outer option match admits exactly these three options"),
                }
                index += 2;
            }
            "--capture" => {
                capture = true;
                index += 1;
            }
            _ => return Err("usage: teaching-game --metadata PATH --snapshot PATH [--case RD01|RD02] [--capture]".into()),
        }
    }
    let metadata = metadata.ok_or("explicit --metadata required")?;
    let snapshot = snapshot.ok_or("explicit --snapshot required")?;
    let source = TeachingSource::load(&metadata, &snapshot, &vocabulary)?;
    if capture {
        let root = current_capture::repository();
        let mut paths = [
            "crates/reference_tools/src/bin/teaching-game.rs",
            "crates/reference_tools/src/lib.rs",
            "crates/reference_tools/src/ordinary_game/mod.rs",
            "crates/reference_tools/src/ordinary_game/teaching.rs",
            "crates/reference_tools/src/ordinary_game/catalogue.rs",
            "crates/reference_tools/src/ordinary_game/teaching-inputs-v1.json",
            "crates/reference_tools/src/ordinary_game/fixture.rs",
            "crates/reference_tools/src/ordinary_game/policy.rs",
            "crates/reference_tools/src/ordinary_game/probes.rs",
            "crates/reference_tools/src/ordinary_game/record.rs",
            "crates/reference_tools/src/ordinary_game/selectors.rs",
            "crates/reference_tools/src/ordinary_game/view.rs",
            "crates/card_programs/src/lib.rs",
            "crates/card_programs/src/teaching.rs",
            "crates/contracts/src/teaching_profile.rs",
        ]
        .map(|p| root.join(p))
        .to_vec();
        paths.extend([metadata, snapshot]);
        let registry =
            serde_json::to_value(source.registry().coverage()).map_err(|e| e.to_string())?;
        current_capture::start("teaching", &paths, Some(&registry))?;
    }
    let report = source.run(&cases, vocabulary)?;
    let completed = report.completed();
    let observations = if completed {
        serde_json::to_value(ApiResponse::<_, ()>::Success { data: report })
    } else {
        serde_json::to_value(ApiResponse::<(), _>::Fail { data: report })
    }
    .map_err(|e| e.to_string())?;
    let output = if capture {
        current_capture::finish("teaching", observations)?
    } else {
        observations
    };
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer(&mut stdout, &output).map_err(|e| e.to_string())?;
    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())?;
    if !completed {
        return Err("teaching trajectory failed; actual report retained on stdout".into());
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("teaching-game failed: {error}");
            ExitCode::FAILURE
        }
    }
}
