//! Actual saved-catalogue mandatory-trigger games over the shared ordinary runner.
use spellwake_contracts::ApiResponse;
use spellwake_reference_tools::{
    creature_types, current_capture,
    ordinary_game::mandatory::{MandatoryCase, MandatorySource},
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
    let mut cases = MandatoryCase::ALL.to_vec();
    let mut capture = false;
    let mut index = 0;
    let mut seen = std::collections::BTreeSet::new();
    while index < args.len() {
        if !seen.insert(args[index].clone()) {
            return Err("repeated mandatory-trigger option".into());
        }
        match args[index].as_str() {
            "--metadata" | "--snapshot" | "--case" => {
                let value = args.get(index + 1).ok_or("missing mandatory-trigger argument value")?;
                match args[index].as_str() {
                    "--metadata" => metadata = Some(PathBuf::from(value)),
                    "--snapshot" => snapshot = Some(PathBuf::from(value)),
                    "--case" => cases = if value == "all" { MandatoryCase::ALL.to_vec() } else { vec![MandatoryCase::parse(value)?] },
                    _ => unreachable!("outer option match admits exactly these three options"),
                }
                index += 2;
            }
            "--capture" => {
                capture = true;
                index += 1;
            }
            _ => return Err("usage: mandatory-trigger-game --metadata PATH --snapshot PATH [--case ID|all] [--capture]".into()),
        }
    }
    let metadata = metadata.ok_or("explicit --metadata required")?;
    let snapshot = snapshot.ok_or("explicit --snapshot required")?;
    let source = MandatorySource::load(&metadata, &snapshot, &vocabulary)?;
    if capture {
        let root = current_capture::repository();
        let mut paths = [
            "crates/reference_tools/src/bin/mandatory-trigger-game.rs",
            "crates/reference_tools/src/ordinary_game/mandatory.rs",
            "crates/reference_tools/src/ordinary_game/mandatory/data.rs",
            "crates/reference_tools/src/ordinary_game/mandatory/driver.rs",
            "crates/reference_tools/src/ordinary_game/mandatory/probes.rs",
            "crates/reference_tools/src/ordinary_game/mandatory-definitions-v1.json",
            "crates/reference_tools/src/ordinary_game/catalogue.rs",
            "crates/reference_tools/src/lib.rs",
            "crates/reference_tools/src/ordinary_game/mod.rs",
            "crates/engine/test_support/error.rs",
            "crates/reference_tools/src/ordinary_game/mandatory-inputs-v1.json",
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
        current_capture::start("mandatory-trigger", &paths, Some(&registry))?;
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
        current_capture::finish("mandatory-trigger", observations)?
    } else {
        observations
    };
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    serde_json::to_writer(&mut stdout, &output).map_err(|e| e.to_string())?;
    stdout.write_all(b"\n").map_err(|e| e.to_string())?;
    stdout.flush().map_err(|e| e.to_string())?;
    if !completed {
        return Err("mandatory-trigger trajectory failed; actual report retained on stdout".into());
    }
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mandatory-trigger-game failed: {error}");
            ExitCode::FAILURE
        }
    }
}
