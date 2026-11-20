//! Explicit remainder/matrix profile; the captured v1 entry and records stay separate.
use super::*;

mod admission;
mod body;
mod construction;
mod definitions;
mod executor;
mod matcher;
mod preflight;
mod raw_window;
mod registry;

#[derive(Debug, PartialEq, Eq)]
enum V2Witness {
    Construction(Box<Result<EffectWindow, EffectError>>),
    Definition {
        validation: Result<(), crate::program::DefinitionError>,
        construction: Box<Result<EffectWindow, EffectError>>,
    },
    Matcher(Box<matcher::MatcherWitness>),
    Registry(Box<registry::RegistryWitness>),
    Executor(Box<executor::ExecutorWitness>),
    ConstructionBatch(Box<construction::ConstructionWitness>),
    RawWindow(Box<raw_window::RawWindowWitness>),
    Body(Box<body::BodyWitness>),
    ResolutionRoot(Box<body::ResolutionRootWitness>),
    Root {
        root: Box<Witness>,
        frame_attempts: Vec<root::FrameAttempt>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Runner {
    Root,
    Body,
    Construction,
    RawWindow,
    Executor,
    Admission,
    Registry,
}

fn runner(selected: &packet::SelectedInput) -> Result<Runner, String> {
    match selected.fixture["id"].as_str() {
        Some(
            "T16-F2-before-return"
            | "T16-F2-after-pending"
            | "T17-M10"
            | "T17-M38-rotation"
            | "T17-M38-same-cleanup"
            | "T17-M39-integrated",
        ) => return Ok(Runner::Root),
        Some(
            "T16-F4-live-control"
            | "T16-F4-departed"
            | "T16-F6-present"
            | "T16-F6-departed"
            | "T17-M19"
            | "T17-M18"
            | "T17-M20-counter-copy"
            | "T17-M21-retain"
            | "T17-M21-change"
            | "T17-M22-normal"
            | "T17-M22-counter"
            | "T17-M22-all-illegal"
            | "T17-M23"
            | "T17-M24"
            | "T17-M25"
            | "T17-M26-multiplier-first"
            | "T17-M26-prevention-first"
            | "T17-M27"
            | "T17-M28"
            | "T17-M29"
            | "T17-M30"
            | "T17-M33",
        ) => return Ok(Runner::Body),
        Some("T17-M01") => return Ok(Runner::Construction),
        Some("T17-M39-raw") => return Ok(Runner::RawWindow),
        Some(
            "T17-M03"
            | "T17-M09-zero"
            | "T17-M09-two"
            | "T17-M09-partial"
            | "T17-M09-huge-empty"
            | "T17-M31"
            | "T17-M32-coalesced-reasons",
        ) => return Ok(Runner::Executor),
        _ => {}
    }
    match selected.fixture["scope"]
        .as_str()
        .ok_or("missing selected scope")?
    {
        "trusted_definition_admission" => Ok(Runner::Admission),
        "trusted_registry" => Ok(Runner::Registry),
        scope => Err(format!(
            "unsupported_scope: {scope} is outside the selected private v2 profile"
        )),
    }
}

fn execute(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    match runner(selected)? {
        Runner::Root => {
            let (report, witness, frame_attempts) = root::run_extended(selected)?;
            Ok((
                report,
                V2Witness::Root {
                    root: Box::new(witness),
                    frame_attempts,
                },
            ))
        }
        Runner::Body => body::run(selected),
        Runner::Construction => construction::run(selected),
        Runner::RawWindow => raw_window::run(selected),
        Runner::Executor => executor::run(selected),
        Runner::Admission => admission::run(selected),
        Runner::Registry => registry::run(selected),
    }
}

fn replay(
    first: Result<(Value, V2Witness), String>,
    second: Result<(Value, V2Witness), String>,
) -> Value {
    let typed_eq = match (&first, &second) {
        (Ok((_, left)), Ok((_, right))) => Some(left == right),
        _ => None,
    };
    let view = |result: Result<(Value, V2Witness), String>| match result {
        Ok((value, _)) => value,
        Err(error) => json!({"status":"failed","failure_domain":"DriverOrCodec","failure":error}),
    };
    let mut first = view(first);
    let second = view(second);
    if let Err(error) = capture::bind(&mut first) {
        return json!({"observation":first,"native_reexecution_integrity":false,"witness_failure":error});
    }
    let record_eq = first == second;
    json!({"observation":first,"reexecution":{"full_native_result_and_state_eq":typed_eq,"full_record_eq":record_eq},"native_reexecution_integrity":typed_eq==Some(true)&&record_eq})
}

fn produce(path: &Path, case: &str, matrix: Option<&Path>) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let matrix_bytes = matrix
        .map(|path| fs::read(path).map_err(|error| error.to_string()))
        .transpose();
    let matrix_digest = matrix_bytes
        .as_ref()
        .ok()
        .and_then(Option::as_ref)
        .map(|value| format!("{:x}", Sha256::digest(value)));
    let observed = (|| -> Result<Value, String> {
        if digest != INPUT_SHA256 {
            return Err("private v2 base input differs from selected B08 bytes".into());
        }
        let packet = packet::Packet::parse(&bytes)?;
        if let Some(matrix_bytes) = matrix_bytes.as_ref().map_err(Clone::clone)? {
            let matrix_hash = matrix_digest.as_deref();
            if !matches!(
                matrix_hash,
                Some(definitions::INPUT_SHA256 | matcher::INPUT_SHA256)
            ) {
                return Err("unsupported or altered v2 matrix input".into());
            }
            let input = packet::unique_json(matrix_bytes)?;
            let matrix_vocabulary =
                creature_types::vocabulary(input["creature_vocabulary"].clone())?;
            let supplied =
                creature_types::load_vocabulary(&creature_types::selected_vocabulary_path()?)?;
            if matrix_vocabulary != *supplied {
                return Err("matrix vocabulary differs from explicitly supplied capture V".into());
            }
            let definitions = packet.definition_inputs()?;
            let run = || match matrix_hash {
                Some(definitions::INPUT_SHA256) => definitions::run(&input, case, &definitions),
                Some(matcher::INPUT_SHA256) => matcher::run(&input, case, &definitions),
                _ => unreachable!("matrix digest was admitted above"),
            };
            capture::begin(path, "ordinary-trigger-private-unit-v2", matrix)?;
            let first = run();
            let second = crate::test_support::current_capture::without_capture(run);
            let mut report = replay(first, second);
            report["selected_matrix"] = input;
            report["selected_definitions"] = json!(definitions);
            report["packet_metadata"] = packet.metadata()?;
            Ok(report)
        } else {
            let selected = packet.select(case)?;
            let explicit =
                creature_types::load_vocabulary(&creature_types::selected_vocabulary_path()?)?;
            if selected.vocabulary != explicit {
                return Err(
                    "selected packet vocabulary differs from explicit capture input".into(),
                );
            }
            capture::begin(path, "ordinary-trigger-private-unit-v2", None)?;
            let first = execute(&selected);
            let second =
                crate::test_support::current_capture::without_capture(|| execute(&selected));
            let mut report = replay(first, second);
            report["selected_input"] = selected.fixture;
            report["selected_definitions"] = json!(selected.definitions);
            report["packet_metadata"] = selected.metadata;
            Ok(report)
        }
    })();
    let after = fs::read(path);
    let unchanged = after.as_ref().is_ok_and(|value| value == &bytes);
    let mut report = match observed {
        Ok(value) => value,
        Err(error) => {
            json!({"observation":{"status":"failed","failure_domain":"DriverOrCodec","failure":error},"native_reexecution_integrity":false})
        }
    };
    report["schema_version"] = json!(crate::test_support::current_profile::REPORT_SCHEMA_VERSION);
    report["record_codec"] = json!("spellwake.current-native-qualification.v2");
    report["profile"] = json!("ordinary-trigger-private-unit-v2");
    report["case"] = json!(case);
    report["input_source"] = json!({"path":path,"role":"base-input-only-recipes-and-definitions","sha256_read":digest,"bytes_read":bytes.len(),"parser_called":digest==INPUT_SHA256,"sha256_parser_input":(digest==INPUT_SHA256).then_some(&digest),"sha256_after":after.as_ref().ok().map(|value|format!("{:x}",Sha256::digest(value))),"after_read_error":after.as_ref().err().map(ToString::to_string),"unchanged":unchanged});
    let matrix_unchanged = match (matrix, matrix_bytes) {
        (None, Ok(None)) => true,
        (Some(path), Ok(Some(before))) => {
            let after = fs::read(path);
            let unchanged = after.as_ref().is_ok_and(|value| value == &before);
            let parser_called = digest == INPUT_SHA256
                && matches!(
                    matrix_digest.as_deref(),
                    Some(definitions::INPUT_SHA256 | matcher::INPUT_SHA256)
                );
            report["matrix_source"] = json!({"path":path,"sha256_read":matrix_digest,"bytes_read":before.len(),"parser_called":parser_called,"sha256_parser_input":parser_called.then_some(&matrix_digest),"sha256_after":after.as_ref().ok().map(|value|format!("{:x}",Sha256::digest(value))),"after_read_error":after.as_ref().err().map(ToString::to_string),"unchanged":unchanged});
            unchanged
        }
        (Some(path), Err(error)) => {
            report["matrix_source"] =
                json!({"path":path,"read_error":error,"parser_called":false,"unchanged":false});
            false
        }
        _ => return Err("inconsistent v2 input selection".into()),
    };
    report["producer_integrity"] =
        json!(report["native_reexecution_integrity"] == true && unchanged && matrix_unchanged);
    report["status"] = json!(if report["producer_integrity"] == true
        && report["observation"]["status"] == "completed_recipe"
    {
        "completed_recipe"
    } else {
        "failed"
    });
    report["qualification_claim"] = json!(
        "none; independent expected checking and captured source/binary/input evidence required"
    );
    capture::finish("ordinary-trigger-private-unit-v2", &mut report);
    Ok(report)
}

pub(super) fn selected_fixture() {
    let path = env::var_os("SPELLWAKE_PRIVATE_INPUT").expect("explicit v2 input path required");
    let case = env::var("SPELLWAKE_PRIVATE_CASE").expect("explicit v2 case required");
    let output = env::var_os("SPELLWAKE_PRIVATE_OUTPUT").expect("explicit v2 output path required");
    let matrix = env::var_os("SPELLWAKE_PRIVATE_MATRIX_INPUT");
    let result = produce(Path::new(&path), &case, matrix.as_deref().map(Path::new));
    let report = match &result {
        Ok(value) => value.clone(),
        Err(error) => {
            json!({"schema_version":crate::test_support::current_profile::REPORT_SCHEMA_VERSION,"record_codec":"spellwake.current-native-qualification.v2","profile":"ordinary-trigger-private-unit-v2","case":case,"status":"failed","failure_domain":"InputRead","failure":error})
        }
    };
    fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).expect("write v2 observations");
    assert!(result.is_ok(), "private v2 producer failed: {result:?}");
    assert_eq!(report["producer_integrity"], true);
    // Successful capture preserves owner errors; the independent checker adjudicates them.
}
