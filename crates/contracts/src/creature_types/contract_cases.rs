//! Actual finite contract calls; expected results are not an input.
use super::*;
use crate::RulesInteger;
use serde_json::{Value, json};
use std::sync::Arc;

#[path = "../../../engine/test_support/creature_vocabulary.rs"]
pub(crate) mod creature_vocabulary;

#[allow(dead_code)]
#[path = "../../../../tests/support/resolution_contract_cases.rs"]
mod support;
use support::{CallRecord, Selection, borrowed_call, input_call, vocabulary_boundary};

#[derive(PartialEq, Eq)]
enum Native {
    Vocabulary(Result<CreatureTypeVocabulary, CreatureVocabularyError>),
    Arcs(Arc<CreatureTypeVocabulary>, Arc<CreatureTypeVocabulary>),
    Member(Option<CreatureTypeId>),
    ForeignMember(CreatureTypeVocabulary, Option<CreatureTypeId>),
    ForeignLabel(CreatureTypeVocabulary, CreatureTypeId, Option<String>),
    Identity(Result<CreatureVocabularyId, InvalidCreatureVocabularyId>),
    Ordinal(Result<u16, CreatureVocabularyError>),
    Count(Result<u32, CreatureVocabularyError>),
    Encoding(Result<u32, CreatureVocabularyError>, Option<[u8; 4]>),
    Integer(RulesInteger),
}

fn encoded<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("native contract serialization")
}
fn vocabulary_result(value: &Result<CreatureTypeVocabulary, CreatureVocabularyError>) -> Value {
    match value {
        Ok(value) => json!({"tag":"Ok","value":vocabulary_boundary::view(value)}),
        Err(error) => json!({"tag":"Err","error":error}),
    }
}
fn member_result(value: &Option<CreatureTypeId>) -> Value {
    match value {
        Some(value) => json!({"tag":"Some","value":vocabulary_boundary::member_view(*value)}),
        None => json!({"tag":"None"}),
    }
}
fn scalar_result<T: ToString>(value: &Result<T, CreatureVocabularyError>) -> Value {
    match value {
        Ok(value) => json!({"tag":"Ok","value":value.to_string()}),
        Err(error) => json!({"tag":"Err","error":error}),
    }
}
fn string(recipe: &Value, key: &str) -> Result<String, String> {
    recipe[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("missing recipe string {key}"))
}
fn decimal<T: std::str::FromStr + ToString>(value: &str) -> Result<T, String> {
    let parsed: T = value
        .parse()
        .map_err(|_| "invalid canonical recipe integer")?;
    if parsed.to_string() != value {
        return Err("noncanonical recipe integer".into());
    }
    Ok(parsed)
}
fn digest(value: &str) -> Result<[u8; 32], String> {
    CreatureVocabularyId::try_from(value.to_owned())
        .map(|value| *value.as_bytes())
        .map_err(|error| error.to_string())
}
type VocabularyArguments = (String, [u8; 32], Vec<String>);
fn arguments_view(value: &VocabularyArguments) -> Value {
    json!({"rules_revision":value.0,"rules_sha256":value.1.iter().map(|byte|format!("{byte:02x}")).collect::<String>(),"members":value.2})
}
fn construct(
    arguments: &VocabularyArguments,
) -> (
    Result<CreatureTypeVocabulary, CreatureVocabularyError>,
    CallRecord,
) {
    input_call(
        "CreatureTypeVocabulary::new",
        arguments,
        |(revision, digest, members)| CreatureTypeVocabulary::new(revision, digest, members),
        arguments_view,
        vocabulary_result,
    )
}
fn full_arguments(selection: &Selection) -> VocabularyArguments {
    let value = &selection.vocabulary;
    (
        value.rules_revision().into(),
        *value.rules_sha256(),
        value.members().to_vec(),
    )
}
fn foreign(
    selection: &Selection,
    calls: &mut Vec<CallRecord>,
) -> Result<CreatureTypeVocabulary, String> {
    let arguments = (
        "SYNTHETIC-SHAPE-FOREIGN".into(),
        [0; 32],
        selection.vocabulary.members().to_vec(),
    );
    let (value, record) = construct(&arguments);
    calls.push(record);
    value.map_err(|error| error.to_string())
}

fn execute(selection: &Selection) -> Result<(Vec<CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    let op = string(recipe, "op")?;
    let full = selection.vocabulary.as_ref();
    let mut calls = Vec::new();
    let native = match op.as_str() {
        "CreatureTypeVocabulary::new" => {
            let mut arguments = if recipe["base"] == "Vfull" || recipe["vocabulary"] == "Vfull" {
                full_arguments(selection)
            } else {
                (
                    string(recipe, "rules_revision")?,
                    digest(&string(recipe, "rules_sha256")?)?,
                    serde_json::from_value(recipe["members"].clone())
                        .map_err(|error| error.to_string())?,
                )
            };
            if let Some(revision) = recipe["metadata_override"]["rules_revision"].as_str() {
                arguments.0 = revision.into();
            }
            if let Some(hash) = recipe["metadata_override"]["rules_sha256"].as_str() {
                arguments.1 = digest(hash)?;
            }
            let transform = &recipe["label_transform"];
            if let Some(label) = transform["remove_label"].as_str() {
                let before = arguments.2.len();
                arguments.2.retain(|member| member != label);
                if arguments.2.len() + 1 != before {
                    return Err("source transform did not remove exactly one member".into());
                }
            }
            if let Some(label) = transform["add_sorted_label"].as_str() {
                arguments.2.push(label.into());
                arguments.2.sort();
            }
            if let Some(old) = transform["replace_label"]["old"].as_str() {
                let mut matches = 0;
                for label in &mut arguments.2 {
                    if label == old {
                        *label = string(&transform["replace_label"], "new")?;
                        matches += 1;
                    }
                }
                if matches != 1 || transform["sort_after_source_transform"] != true {
                    return Err("invalid exact source label transform".into());
                }
                arguments.2.sort();
            }
            let (result, record) = construct(&arguments);
            calls.push(record);
            Native::Vocabulary(result)
        }
        "construct separate Vfull values and wrap each in separate Arc allocation" => {
            let (left, first) = construct(&full_arguments(selection));
            calls.push(first);
            let (right, second) = construct(&full_arguments(selection));
            calls.push(second);
            let pair = (
                Arc::new(left.map_err(|error| error.to_string())?),
                Arc::new(right.map_err(|error| error.to_string())?),
            );
            let pair_view = |pair: &(Arc<CreatureTypeVocabulary>, Arc<CreatureTypeVocabulary>)| json!({"left":vocabulary_boundary::view(&pair.0),"right":vocabulary_boundary::view(&pair.1)});
            let (_, equality) = input_call(
                "Arc::eq",
                &pair,
                |(left, right)| left == right,
                pair_view,
                encoded,
            );
            calls.push(equality);
            let (_, identity) = input_call(
                "Arc::ptr_eq",
                &pair,
                |(left, right)| Arc::ptr_eq(&left, &right),
                pair_view,
                encoded,
            );
            calls.push(identity);
            Native::Arcs(pair.0, pair.1)
        }
        "Vfull.resolve_member" => {
            let mut foreign_value = None;
            let identity = if recipe["vocabulary"] == "Vforeign.id" {
                let value = foreign(selection, &mut calls)?;
                let id = value.id();
                foreign_value = Some(value);
                id
            } else {
                CreatureVocabularyId::try_from(string(recipe, "vocabulary")?)
                    .map_err(|error| error.to_string())?
            };
            let ordinal = decimal::<u16>(&string(recipe, "ordinal_u16")?)?;
            let (result, record) = borrowed_call(
                "CreatureTypeVocabulary::resolve_member",
                full,
                &(identity, ordinal),
                |source, args| source.resolve_member(args.0, args.1),
                vocabulary_boundary::view,
                |args| json!({"vocabulary":args.0.to_string(),"ordinal":args.1.to_string()}),
                member_result,
            );
            calls.push(record);
            match foreign_value {
                Some(foreign) => Native::ForeignMember(foreign, result),
                None => Native::Member(result),
            }
        }
        "Vfull.resolve_label" => {
            let label = string(recipe, "label")?;
            let (result, record) = borrowed_call(
                "CreatureTypeVocabulary::resolve_label",
                full,
                &label,
                |source, label| source.resolve_label(label),
                vocabulary_boundary::view,
                encoded,
                member_result,
            );
            calls.push(record);
            Native::Member(result)
        }
        "Vfull.label" => {
            let foreign = foreign(selection, &mut calls)?;
            if recipe["id"]["from"] != "Vforeign.resolve_label"
                || recipe["id"]["require_some"] != true
            {
                return Err("wrong foreign ID construction recipe".into());
            }
            let label = string(&recipe["id"], "label")?;
            let (id, lookup) = borrowed_call(
                "CreatureTypeVocabulary::resolve_label",
                &foreign,
                &label,
                |source, label| source.resolve_label(label),
                vocabulary_boundary::view,
                encoded,
                member_result,
            );
            calls.push(lookup);
            let id = id.ok_or("explicit foreign member lookup failed")?;
            let (result, record) = borrowed_call(
                "CreatureTypeVocabulary::label",
                full,
                &id,
                |source, id| source.label(*id),
                vocabulary_boundary::view,
                |id| vocabulary_boundary::member_view(*id),
                |result| match result {
                    Some(label) => json!({"tag":"Some","value":label}),
                    None => json!({"tag":"None"}),
                },
            );
            calls.push(record);
            Native::ForeignLabel(foreign, id, result.map(str::to_owned))
        }
        "actual source-used checked_member_ordinal(usize) helper" => {
            let arguments = decimal::<usize>(&string(recipe, "index_as_usize_decimal")?)?;
            let (result, record) = input_call(
                "checked_member_ordinal",
                &arguments,
                checked_member_ordinal,
                |value| json!({"usize":value.to_string()}),
                scalar_result,
            );
            calls.push(record);
            Native::Ordinal(result)
        }
        "actual source-used checked_member_count(usize) helper returning u32" => {
            let arguments = decimal::<usize>(&string(recipe, "count_as_usize_decimal")?)?;
            let (result, record) = input_call(
                "checked_member_count",
                &arguments,
                checked_member_count,
                |value| json!({"usize":value.to_string()}),
                scalar_result,
            );
            calls.push(record);
            Native::Count(result)
        }
        "actual source-used checked_encoding_length(usize), then to_be_bytes on the Ok u32 result" =>
        {
            let arguments = decimal::<usize>(&string(recipe, "length_as_u64_decimal")?)?;
            let (result, record) = input_call(
                "checked_encoding_length",
                &arguments,
                checked_encoding_length,
                |value| json!({"usize":value.to_string()}),
                scalar_result,
            );
            calls.push(record);
            let bytes = if let Ok(value) = result {
                let (bytes, record) = input_call(
                    "u32::to_be_bytes",
                    &value,
                    u32::to_be_bytes,
                    |value| json!({"u32":value.to_string()}),
                    encoded,
                );
                calls.push(record);
                Some(bytes)
            } else {
                None
            };
            Native::Encoding(result, bytes)
        }
        "RulesInteger::from_usize(usize)" => {
            if recipe["target_width_bits"] != usize::BITS {
                return Err("actual target usize width differs from selected recipe".into());
            }
            let arguments = decimal::<usize>(&string(recipe, "usize_decimal")?)?;
            let (result, record) = input_call(
                "RulesInteger::from_usize",
                &arguments,
                RulesInteger::from_usize,
                |value| json!({"usize":value.to_string(),"target_width_bits":usize::BITS}),
                encoded,
            );
            calls.push(record);
            Native::Integer(result)
        }
        "CreatureVocabularyId::try_from(String)" => {
            let arguments = string(recipe, "raw")?;
            let (result, record) = input_call(
                "CreatureVocabularyId::try_from(String)",
                &arguments,
                CreatureVocabularyId::try_from,
                encoded,
                |result| match result {
                    Ok(value) => json!({"tag":"Ok","value":value.to_string()}),
                    Err(_) => json!({"tag":"Err","error":{"tag":"InvalidCreatureVocabularyId"}}),
                },
            );
            calls.push(record);
            Native::Identity(result)
        }
        _ => return Err(format!("unsupported contracts-owned operation {op}")),
    };
    Ok((calls, native))
}

#[test]
#[ignore = "explicit source-bound finite contract capture only"]
fn selected_contract_case() {
    let selected = Selection::from_env(OWNED_IDS).expect("strict finite contract admission");
    support::run(&selected, || execute(&selected)).expect("actual typed finite contract calls");
}

const OWNED_IDS: &[&str] = &[
    "S2C-VM02-001",
    "S2C-VM02-002",
    "S2C-VM02-003",
    "S2C-VM02-004",
    "S2C-VM02-005",
    "S2C-VM02-006",
    "S2C-VM02-007",
    "S2C-VM02-008",
    "S2C-VM02-009",
    "S2C-VM02-010",
    "S2C-VM02-011",
    "S2C-VM02-012",
    "S2C-VM02-013",
    "S2C-VM02-014",
    "S2C-VM02-015",
    "S2C-VM02-016",
    "S2C-VM02-017",
    "S2C-VM02-018",
    "S2C-VM02-019",
    "S2C-VM02-020",
    "S2C-VM02-021",
    "S2C-VM02-022",
    "S2C-VM02-023",
    "S2C-VM02-024",
    "S2C-VM03-001",
    "S2C-VM03-002",
    "S2C-VM03-003",
    "S2C-VM03-004",
    "S2C-VM03-005",
    "S2C-VM03-006",
    "S2C-VM08-001",
    "S2C-VM08-002",
    "S2C-VM08-003",
    "S2C-VM08-004",
    "S2C-VM08-005",
    "S2C-VM08-006",
    "S2C-VM08-007",
    "S2C-VM08-008",
    "S2C-VM29-001",
    "S2C-VM29-002",
    "S2C-VM29-003",
    "S2C-VM29-004",
    "S2C-VM29-005",
    "S2C-VM29-006",
    "S2C-VM29-007",
    "S2C-VM29-008",
    "S2C-VM29-009",
    "S2C-VM29-010",
    "S2C-VM30-043",
    "S2C-VM30-044",
    "S2C-VM30-045",
    "S2C-VM30-046",
    "S2C-VM30-047",
];
