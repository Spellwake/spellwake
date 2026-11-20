//! Shared finite-input admission and actual-call records for owning test modules.
//! No rule evaluation, expected-result input, native decoder or semantic checker.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::{env, fs, io::Write, path::PathBuf, sync::Arc};

pub(crate) use super::creature_vocabulary as vocabulary_boundary;

pub const INPUT_SHA256: &str = "0a9dd9403af5c76e1d952e8d77fe40850f4840df6f9dd9bdc699db24ab93dafd";
const PUBLICATION_SHA256: &str = "29950c2ce036e1bcee6b9a86d9947294a11ef297a2cdcdc75edbcc0b5de40253";
const VOCABULARY_INPUT_SHA256: &str =
    "dd16c29df30b69a72ea9021f0e60429a8537914e9e85187d36559e42ce59cd3c";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub case_id: String,
    pub families: Vec<String>,
    pub scope: String,
    pub recipe: Value,
    pub recipe_sha256: String,
    pub authority_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRef {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

pub struct Selection {
    pub case: Case,
    pub vocabulary: Arc<CreatureTypeVocabulary>,
    pub input: SourceRef,
    pub vocabulary_input: SourceRef,
    pub publication: SourceRef,
    output: PathBuf,
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn selected_file(variable: &str) -> Result<(PathBuf, Vec<u8>, SourceRef), String> {
    let path = PathBuf::from(env::var_os(variable).ok_or_else(|| format!("{variable} required"))?);
    let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let binding = SourceRef {
        path: path.to_string_lossy().into_owned(),
        sha256: hash(&bytes),
        bytes: bytes
            .len()
            .try_into()
            .map_err(|_| "source length overflow")?,
    };
    Ok((path, bytes, binding))
}

impl Selection {
    pub fn from_env(owned_ids: &[&str]) -> Result<Self, String> {
        let (_, bytes, input) = selected_file("SPELLWAKE_CONTRACT_INPUT")?;
        if input.sha256 != INPUT_SHA256 {
            return Err("contract input differs from selected 221-case file".into());
        }
        // Admission hashes the exact sealed file before parsing; it is not a raw codec probe.
        let packet: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if packet["schema_version"] != 1
            || packet["kind"] != "S2-input-only-contract-case-preparation"
        {
            return Err("wrong finite contract packet".into());
        }
        let rows = packet["cases"]
            .as_array()
            .ok_or("contract cases must be an array")?;
        if rows.len() != 221 {
            return Err("exactly 221 contract recipes required".into());
        }
        let selected =
            env::var("SPELLWAKE_CONTRACT_CASE").map_err(|_| "SPELLWAKE_CONTRACT_CASE required")?;
        if !owned_ids.contains(&selected.as_str()) {
            return Err("case is not owned by this exact native entry".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut found = None;
        for row in rows {
            let case: Case =
                serde_json::from_value(row.clone()).map_err(|error| error.to_string())?;
            if !seen.insert(case.case_id.clone()) {
                return Err("duplicate finite case ID".into());
            }
            let mut recipe_input = row.clone();
            recipe_input
                .as_object_mut()
                .ok_or("case must be an object")?
                .remove("recipe_sha256");
            let encoded = serde_json::to_vec(&recipe_input).map_err(|error| error.to_string())?;
            if hash(&encoded) != case.recipe_sha256 {
                return Err(format!("recipe hash mismatch: {}", case.case_id));
            }
            if case.case_id == selected {
                found = Some(case);
            }
        }
        let case = found.ok_or("selected finite case absent")?;
        let (vocabulary_path, _, vocabulary_input) =
            selected_file("SPELLWAKE_CREATURE_VOCABULARY_INPUT")?;
        if vocabulary_input.sha256 != VOCABULARY_INPUT_SHA256 {
            return Err("explicit vocabulary is not the selected actual D06 flat companion".into());
        }
        let vocabulary = vocabulary_boundary::load_vocabulary(&vocabulary_path)?;
        let (_, _, publication) = selected_file("SPELLWAKE_CREATURE_VOCABULARY_PUBLICATION")?;
        if publication.sha256 != PUBLICATION_SHA256 {
            return Err("explicit publication is not the accepted actual D06 output".into());
        }
        let output = PathBuf::from(
            env::var_os("SPELLWAKE_CONTRACT_OUTPUT").ok_or("SPELLWAKE_CONTRACT_OUTPUT required")?,
        );
        Ok(Self {
            case,
            vocabulary,
            input,
            vocabulary_input,
            publication,
            output,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SourceCapture {
    BeforeInvocation,
    TypedInputOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallRecord {
    pub method: String,
    pub arguments: Value,
    pub replayed_arguments: Value,
    pub source_capture: SourceCapture,
    pub source_before: Option<Value>,
    pub source_after: Option<Value>,
    pub result: Value,
    pub replayed_result: Value,
    pub complete_typed_result_eq: bool,
    pub immutable_source_eq: Option<bool>,
    pub typed_input_retained: bool,
}

/// Borrowed owners are cloned before either call; equality uses actual native types.
pub fn borrowed_call<'a, S: Clone + Eq, A: Clone + Eq, R: Eq>(
    method: &str,
    source: &'a S,
    arguments: &A,
    invoke: impl Fn(&'a S, &A) -> R,
    source_view: impl Fn(&S) -> Value,
    arguments_view: impl Fn(&A) -> Value,
    result_view: impl Fn(&R) -> Value,
) -> (R, CallRecord) {
    let saved_source = source.clone();
    let saved_input = arguments.clone();
    let source_before = source_view(&saved_source);
    let arguments_before = arguments_view(&saved_input);
    let result = invoke(source, &saved_input);
    let replayed_arguments = arguments_view(&saved_input);
    let replayed = invoke(source, &saved_input);
    let record = CallRecord {
        method: method.into(),
        arguments: arguments_before,
        replayed_arguments,
        source_capture: SourceCapture::BeforeInvocation,
        source_before: Some(source_before),
        source_after: Some(source_view(source)),
        result: result_view(&result),
        replayed_result: result_view(&replayed),
        complete_typed_result_eq: result == replayed,
        immutable_source_eq: Some(*source == saved_source),
        typed_input_retained: *arguments == saved_input,
    };
    (result, record)
}

/// Consuming calls retain their typed input; they claim no previous owner state.
pub fn input_call<A: Clone + Eq, R: Eq>(
    method: &str,
    arguments: &A,
    invoke: impl Fn(A) -> R,
    arguments_view: impl Fn(&A) -> Value,
    result_view: impl Fn(&R) -> Value,
) -> (R, CallRecord) {
    let saved = arguments.clone();
    let arguments_before = arguments_view(&saved);
    let result = invoke(saved.clone());
    let replayed_arguments = arguments_view(&saved);
    let replayed = invoke(saved.clone());
    let record = CallRecord {
        method: method.into(),
        arguments: arguments_before,
        replayed_arguments,
        source_capture: SourceCapture::TypedInputOnly,
        source_before: None,
        source_after: None,
        result: result_view(&result),
        replayed_result: result_view(&replayed),
        complete_typed_result_eq: result == replayed,
        immutable_source_eq: None,
        typed_input_retained: *arguments == saved,
    };
    (result, record)
}

/// Reexecute the full recipe and compare its actual typed final value separately.
pub fn run<N: Eq>(
    selection: &Selection,
    recipe: impl FnMut() -> Result<(Vec<CallRecord>, N), String>,
) -> Result<(), String> {
    run_with_finish(selection, recipe, Ok)
}

/// Engine owners attach their existing current witness before the single write.
pub fn run_with_finish<N: Eq>(
    selection: &Selection,
    mut recipe: impl FnMut() -> Result<(Vec<CallRecord>, N), String>,
    finish: impl FnOnce(Value) -> Result<Value, String>,
) -> Result<(), String> {
    let (calls, final_native) = recipe()?;
    let (replayed_calls, replayed_final_native) = recipe()?;
    let complete_recipe_record_eq = calls == replayed_calls;
    let final_native_state_eq = final_native == replayed_final_native;
    if calls.is_empty() {
        return Err("successful zero-call recipe is prohibited".into());
    }
    let all_call_evidence = calls.iter().all(|call| {
        call.complete_typed_result_eq
            && call.typed_input_retained
            && call.immutable_source_eq != Some(false)
    });
    let record = finish(json!({
        "schema_version":3,"record_codec":"spellwake.current-native-qualification.v2",
        "profile":"resolution-contract-cases","record_kind":"ContractCase",
        "case":selection.case.case_id,"families":selection.case.families,"scope":selection.case.scope,
        "input":selection.input,"recipe_sha256":selection.case.recipe_sha256,
        "vocabulary_witness":{"value":vocabulary_boundary::view(&selection.vocabulary),
            "input":selection.vocabulary_input,"publication":selection.publication},
        "calls":calls,"replayed_calls":replayed_calls,
        "complete_recipe_record_eq":complete_recipe_record_eq,
        "final_native_state_eq":final_native_state_eq
    }))?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&selection.output)
        .map_err(|error| format!("{}: {error}", selection.output.display()))?;
    serde_json::to_writer(&mut output, &record).map_err(|error| error.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())?;
    if !all_call_evidence || !complete_recipe_record_eq || !final_native_state_eq {
        return Err(
            "actual native call/source/recipe replay equality failed; raw record retained".into(),
        );
    }
    Ok(())
}
