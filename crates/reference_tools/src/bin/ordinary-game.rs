//! Actual opening-to-outcome native trajectories using compiled input recipes.
//! No expected, reference, previous result or runtime fixture file is read.
use serde::Serialize;
use spellwake_contracts::ApiResponse;
use spellwake_reference_tools::ordinary_game::{fixture::CaseId, run_cases};
use std::{
    collections::BTreeMap,
    io::{self, BufWriter, Write},
    process::ExitCode,
};

fn emit_to(value: &impl Serialize, output: impl Write) -> Result<(), serde_json::Error> {
    let mut output = BufWriter::new(output);
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n").map_err(serde_json::Error::io)?;
    output.flush().map_err(serde_json::Error::io)
}
fn emit(value: &impl Serialize) -> Result<(), serde_json::Error> {
    emit_to(value, io::stdout().lock())
}
/// Preserve the actual inner observation while reporting current capture failure.
fn capture_failure(observations: serde_json::Value, error: String) -> serde_json::Value {
    serde_json::json!({"schema_version":spellwake_reference_tools::current_profile::REPORT_SCHEMA_VERSION,
        "record_codec":"spellwake.current-native-qualification.v2","profile":"ordinary",
        "observations":observations,"capture_failure":error})
}
fn main() -> ExitCode {
    let vocabulary = match spellwake_reference_tools::creature_types::selected_vocabulary() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            return ExitCode::FAILURE;
        }
    };
    let args = match spellwake_reference_tools::creature_types::qualification_args() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("invalid qualification input: {error}");
            return ExitCode::FAILURE;
        }
    };
    let cases = match args.as_slice() {
        [] => Ok(CaseId::ALL.to_vec()),
        [flag, id] if flag == "--case" => CaseId::parse(id).map(|case| vec![case]),
        _ => Err("usage: ordinary-game [--case G01|G02a|G02b|G03]".into()),
    };
    let result = match cases {
        Ok(cases) => {
            let root = spellwake_reference_tools::current_capture::repository();
            let sources = [
                "crates/reference_tools/src/bin/ordinary-game.rs",
                "crates/reference_tools/src/ordinary_game/fixture.rs",
                "crates/reference_tools/src/ordinary_game/policy.rs",
                "crates/reference_tools/src/ordinary_game/probes.rs",
                "crates/reference_tools/src/ordinary_game/record.rs",
                "crates/reference_tools/src/ordinary_game/selectors.rs",
                "crates/reference_tools/src/ordinary_game/view.rs",
            ]
            .map(|path| root.join(path));
            if let Err(error) =
                spellwake_reference_tools::current_capture::start("ordinary", &sources, None)
            {
                eprintln!("ordinary capture setup failed: {error}");
                return ExitCode::FAILURE;
            }
            let report = run_cases(&cases, vocabulary);
            let completed = report.completed();
            let observations = if completed {
                serde_json::to_value(ApiResponse::<_, ()>::Success { data: report })
            } else {
                serde_json::to_value(ApiResponse::<(), _>::Fail { data: report })
            };
            observations.and_then(|observations| {
                match spellwake_reference_tools::current_capture::finish(
                    "ordinary",
                    observations.clone(),
                ) {
                    Ok(envelope) => emit(&envelope).map(|()| completed),
                    Err(error) => {
                        eprintln!("ordinary capture failed: {error}");
                        emit(&capture_failure(observations, error)).map(|()| false)
                    }
                }
            })
        }
        Err(message) => emit(&ApiResponse::<(), _>::Fail {
            data: BTreeMap::from([("arguments", message)]),
        })
        .map(|()| false),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("ordinary-game serialization failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit existing actual report path required for byte-equivalence proof; serialization only, no native execution"]
    fn streamed_emission_preserves_existing_report_compact_bytes() {
        let path = std::env::var_os("SPELLWAKE_ORDINARY_BYTE_EQ_INPUT")
            .expect("explicit existing actual report required");
        let value: serde_json::Value =
            serde_json::from_reader(std::fs::File::open(path).unwrap()).unwrap();
        let mut previous = serde_json::to_string(&value).unwrap().into_bytes();
        previous.push(b'\n');
        let mut streamed = Vec::new();
        emit_to(&value, &mut streamed).unwrap();
        assert_eq!(streamed, previous);
    }
    #[test]
    fn current_capture_failure_writer_preserves_the_independent_inner_record() {
        let inner = serde_json::json!({"status":"fail","data":{"schema_version":1,"actual_field":["retained"]}});
        let report = capture_failure(inner.clone(), "unit capture failure".into());
        let mut bytes = Vec::new();
        emit_to(&report, &mut bytes).unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(actual["schema_version"], 10);
        assert_eq!(
            actual["record_codec"],
            "spellwake.current-native-qualification.v2"
        );
        assert_eq!(actual["profile"], "ordinary");
        assert_eq!(actual["observations"], inner);
        assert_eq!(actual["capture_failure"], "unit capture failure");
    }
    #[test]
    fn explicit_flush_failure_is_returned() {
        struct FlushFailure;
        impl Write for FlushFailure {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "deliberate test flush failure",
                ))
            }
        }
        let error = emit_to(&serde_json::json!({"status":"fail"}), FlushFailure).unwrap_err();
        assert!(error.is_io());
    }
}
