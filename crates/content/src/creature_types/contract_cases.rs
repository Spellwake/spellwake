//! Input-only selected calls to the actual content compiler and private helpers.
use super::*;
use serde_json::{Value, json};

#[path = "../../../engine/test_support/creature_vocabulary.rs"]
pub(crate) mod creature_vocabulary;

#[allow(dead_code)]
#[path = "../../../../tests/support/resolution_contract_cases.rs"]
mod support;
use support::{CallRecord, Selection, borrowed_call, input_call};

const PINNED_LINE: &str = include_str!("../../tests/fixtures/creature-section.input.txt");

#[derive(PartialEq, Eq)]
enum Native {
    Compilation(Result<CreatureTypeCompilation, CreatureTypeCompileError>),
    Location(Result<(String, String, usize, usize), SectionFailure>),
    Parsing(Result<ParsedSection, SectionFailure>),
    Utf8(Result<String, CreatureTypeCompileError>),
    Writing(Result<(), CreatureTypeCliError>),
}
fn string(value: &Value, key: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("missing recipe string {key}"))
}
fn encoded<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("native content serialization")
}
fn failure(value: &SectionFailure) -> Value {
    json!({"type":"SectionFailure","reason":value.reason,"diagnostics":value.diagnostics})
}
fn compile_result(value: &Result<CreatureTypeCompilation, CreatureTypeCompileError>) -> Value {
    match value {
        Ok(value) => json!({"tag":"Ok","value":value}),
        Err(error) => json!({"tag":"Err","error":error}),
    }
}
fn source_bytes() -> Result<Vec<u8>, String> {
    let path = std::env::var_os("SPELLWAKE_CONTRACT_CR_INPUT")
        .ok_or("SPELLWAKE_CONTRACT_CR_INPUT required")?;
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if support::hash(&bytes) != hex(&RULES_SHA256) {
        return Err("CR input is not the exact selected source".into());
    }
    Ok(bytes)
}
fn replace_once(bytes: &mut Vec<u8>, old: &str, new: &str) -> Result<(), String> {
    if old.is_empty() {
        return Err("empty transform needle".into());
    }
    let offset = bytes
        .windows(old.len())
        .position(|part| part == old.as_bytes())
        .ok_or("literal source transform not found")?;
    bytes.splice(offset..offset + old.len(), new.bytes());
    Ok(())
}
fn transformed_source(recipe: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = source_bytes()?;
    let transform = &recipe["transform"];
    if !transform.is_null() {
        match string(transform, "kind")?.as_str() {
            "replace_once" => replace_once(
                &mut bytes,
                &string(transform, "old")?,
                &string(transform, "new")?,
            )?,
            "xor_byte" => {
                let offset = string(transform, "offset")?
                    .parse::<usize>()
                    .map_err(|_| "invalid byte offset")?;
                let mask = string(transform, "mask")?
                    .parse::<u8>()
                    .map_err(|_| "invalid byte mask")?;
                *bytes.get_mut(offset).ok_or("byte offset out of bounds")? ^= mask;
            }
            "replace_byte" => {
                let offset = string(transform, "offset")?
                    .parse::<usize>()
                    .map_err(|_| "invalid byte offset")?;
                let value = u8::from_str_radix(&string(transform, "value_hex")?, 16)
                    .map_err(|_| "invalid byte literal")?;
                *bytes.get_mut(offset).ok_or("byte offset out of bounds")? = value;
            }
            _ => return Err("unsupported literal source transform".into()),
        }
    }
    if let Some(expected) = transform["mutated_input_sha256"].as_str()
        && support::hash(&bytes) != expected
    {
        return Err("literal source mutation SHA differs".into());
    }
    Ok(bytes)
}
fn synthetic_section(recipe: &Value) -> Result<String, String> {
    if recipe["source"] != "PinnedSectionLine" || recipe["production_compile_not_called"] != true {
        return Err("not an explicitly synthetic section-helper recipe".into());
    }
    let mut bytes = PINNED_LINE.as_bytes().to_vec();
    let transform = &recipe["synthetic_transform"];
    if let Some(append) = transform["append"].as_str() {
        bytes.extend_from_slice(append.as_bytes());
    } else {
        replace_once(
            &mut bytes,
            &string(transform, "old")?,
            &string(transform, "new")?,
        )?;
    }
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn execute(selection: &Selection) -> Result<(Vec<CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    let op = string(recipe, "op")?;
    let mut calls = Vec::new();
    let native = match op.as_str() {
        "content.compile" => {
            if recipe["source"] != "PinnedCR" {
                return Err("unsupported source selection".into());
            }
            let digest = spellwake_contracts::creature_types::CreatureVocabularyId::try_from(
                string(recipe, "declared_sha256")?,
            )
            .map_err(|error| error.to_string())?;
            let arguments = (
                transformed_source(recipe)?,
                CreatureTypeSource {
                    rules_revision: string(recipe, "declared_revision")?,
                    rules_sha256: *digest.as_bytes(),
                },
            );
            let (result, record) = input_call(
                "content::creature_types::compile",
                &arguments,
                |(bytes, source)| compile(&bytes, source),
                |args| json!({"raw_hex":hex(&args.0),"raw_sha256":support::hash(&args.0),"declared_source":args.1}),
                compile_result,
            );
            calls.push(record);
            Native::Compilation(result)
        }
        "cfg_test_selected_section_locator" => {
            if recipe["production_compile_not_called"] != true {
                return Err("not a synthetic locator case".into());
            }
            let lines = recipe["synthetic_buffer"]["lines"]
                .as_array()
                .ok_or("locator lines required")?;
            let mut buffer = String::new();
            for line in lines {
                let line = line.as_str().ok_or("locator line must be text")?;
                if line == "PinnedSectionLine+LF" {
                    buffer.push_str(PINNED_LINE);
                    buffer.push('\n');
                } else {
                    buffer.push_str(line);
                }
            }
            let (result, record) = borrowed_call(
                "content::creature_types::locate_creature_section",
                &buffer,
                &(),
                |source, ()| locate_creature_section(source),
                encoded,
                |()| json!({"scope":"synthetic-locator-only","production_compile_called":false}),
                |result| match result {
                    Ok(value) => {
                        json!({"tag":"Ok","value":{"line":value.line,"line_ending":value.line_ending,"physical_lf_line":value.physical_lf_line.to_string(),"source_byte_start":value.source_byte_start.to_string()}})
                    }
                    Err(error) => json!({"tag":"Err","error":failure(error)}),
                },
            );
            calls.push(record);
            Native::Location(result.map(|value| {
                (
                    value.line.into(),
                    value.line_ending.into(),
                    value.physical_lf_line,
                    value.source_byte_start,
                )
            }))
        }
        "cfg_test_selected_section_parser" => {
            let source = synthetic_section(recipe)?;
            let (result, record) = borrowed_call(
                "content::creature_types::parse_creature_section",
                &source,
                &(),
                |source, ()| parse_creature_section(source),
                encoded,
                |()| json!({"scope":"synthetic-section-parser-only","production_compile_called":false}),
                |result| match result {
                    Ok(value) => {
                        json!({"tag":"Ok","value":{"members":value.members,"tokens":value.tokens}})
                    }
                    Err(error) => json!({"tag":"Err","error":failure(error)}),
                },
            );
            calls.push(record);
            Native::Parsing(result)
        }
        "std::str::from_utf8 via content decoding helper" => {
            if recipe["production_compile_not_called"] != true || recipe["raw_hex"] != "ff" {
                return Err("unsupported selected UTF8 helper recipe".into());
            }
            let source = vec![0xff];
            let (result, record) = borrowed_call(
                "content::creature_types::decode_source_utf8",
                &source,
                &(),
                |source, ()| decode_source_utf8(source),
                |bytes| json!({"raw_hex":hex(bytes)}),
                |()| json!({"scope":"synthetic-decoder-only","production_compile_called":false}),
                |result| match result {
                    Ok(value) => json!({"tag":"Ok","value":value}),
                    Err(error) => json!({"tag":"Err","error":error}),
                },
            );
            calls.push(record);
            Native::Utf8(result.map(str::to_owned))
        }
        "serde_json::to_writer through selected CLI output helper" => {
            if recipe["prepared_dto"]
                != "well-formed Complete DTO fixture matching proposed schema, not compiler output"
                || recipe["writer"] != "test writer always returns std::io::ErrorKind::BrokenPipe"
            {
                return Err("unsupported exact output-helper recipe".into());
            }
            struct Broken;
            impl io::Write for Broken {
                fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                    Err(io::ErrorKind::BrokenPipe.into())
                }
                fn flush(&mut self) -> io::Result<()> {
                    Err(io::ErrorKind::BrokenPipe.into())
                }
            }
            let prepared = super::tests::prepared_dto();
            let (result, record) = input_call(
                "content::creature_types::write_output",
                &prepared,
                |value| write_output(Broken, &ApiResponse::Success { data: value }),
                |value| json!({"response":ApiResponse::<&Value>::Success{data:value},"writer":{"tag":"AlwaysBrokenPipe"},"prepared_DTO_is_compiler_output":false}),
                |result| match result {
                    Ok(()) => json!({"tag":"Ok","value":null}),
                    Err(error) => match error {
                        CreatureTypeCliError::Read { io_kind } => {
                            json!({"tag":"Err","error":{"tag":"Read","io_kind":format!("{io_kind:?}")}})
                        }
                        CreatureTypeCliError::Serialization { io_kind } => {
                            json!({"tag":"Err","error":{"tag":"Serialization","io_kind":format!("{io_kind:?}")}})
                        }
                    },
                },
            );
            calls.push(record);
            Native::Writing(result)
        }
        _ => return Err(format!("unsupported content-owned operation {op}")),
    };
    Ok((calls, native))
}

#[test]
#[ignore = "explicit source-bound finite contract capture only"]
fn selected_contract_case() {
    let selected = Selection::from_env(OWNED_IDS).expect("strict finite content admission");
    support::run(&selected, || execute(&selected)).expect("actual typed finite content calls");
}

const OWNED_IDS: &[&str] = &[
    "S2C-VM01-001",
    "S2C-VM01-002",
    "S2C-VM01-003",
    "S2C-VM01-004",
    "S2C-VM01-005",
    "S2C-VM01-006",
    "S2C-VM01-007",
    "S2C-VM01-008",
    "S2C-VM01-010",
    "S2C-VM01-011",
    "S2C-VM01-012",
    "S2C-VM01-013",
    "S2C-VM01-014",
    "S2C-VM01-015",
    "S2C-VM01-016",
    "S2C-VM01-017",
    "S2C-VM01-018",
    "S2C-VM01-019",
];
