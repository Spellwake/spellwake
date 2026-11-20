//! Strict shared syntax and membership checks for qualification-only type inputs.
use serde::Deserialize;
use serde_json::Value;
use spellwake_contracts::creature_types::{
    CreatureTypeId, CreatureTypeVocabulary, CreatureVocabularyId,
};
use spellwake_engine::program::CreatureTypeSlot;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    vocabulary: String,
    ordinal: String,
}

fn ordinal(value: &str) -> Result<u16, String> {
    let parsed = value
        .parse::<u16>()
        .map_err(|_| "expected a canonical unsigned u16 decimal string".to_owned())?;
    if parsed.to_string() != value {
        return Err("expected a canonical unsigned u16 decimal string".into());
    }
    Ok(parsed)
}

/// Decode the exact ID object and resolve it against the supplied vocabulary.
/// Raw callers must use `packet::unique_json` first to reject repeated JSON keys.
pub fn id(value: Value, vocabulary: &CreatureTypeVocabulary) -> Result<CreatureTypeId, String> {
    let raw: Id = serde_json::from_value(value).map_err(|error| error.to_string())?;
    let vocabulary_id =
        CreatureVocabularyId::try_from(raw.vocabulary).map_err(|error| format!("{error:?}"))?;
    vocabulary
        .resolve_member(vocabulary_id, ordinal(&raw.ordinal)?)
        .ok_or_else(|| "creature type ID is not a member of the supplied vocabulary".to_owned())
}

/// Decode syntax only; executable-body validation owns producer/use admission.
pub fn slot(value: Value) -> Result<CreatureTypeSlot, String> {
    let raw = value
        .as_str()
        .ok_or("slot must be a canonical unsigned u16 decimal string")?;
    Ok(CreatureTypeSlot::new(ordinal(raw)?))
}

pub use super::creature_vocabulary::{load_vocabulary, vocabulary};

fn explicit_arguments() -> Result<(std::path::PathBuf, Vec<String>), String> {
    let mut arguments = std::env::args().skip(1);
    let mut cli = None;
    let mut remaining = Vec::new();
    while let Some(argument) = arguments.next() {
        if argument == "--creature-vocabulary-input" {
            let path = arguments
                .next()
                .ok_or("--creature-vocabulary-input requires a path")?;
            if cli.replace(path).is_some() {
                return Err("repeated --creature-vocabulary-input".into());
            }
        } else {
            remaining.push(argument);
        }
    }
    let environment = std::env::var("SPELLWAKE_CREATURE_VOCABULARY_INPUT").ok();
    if let (Some(cli), Some(environment)) = (&cli, &environment)
        && std::path::Path::new(cli) != std::path::Path::new(environment)
    {
        return Err("conflicting explicit vocabulary inputs".into());
    }
    let path = cli.or(environment).ok_or(
        "qualification requires --creature-vocabulary-input or SPELLWAKE_CREATURE_VOCABULARY_INPUT",
    )?;
    Ok((path.into(), remaining))
}
/// The exact explicitly selected input path, without reading any other source.
pub fn selected_vocabulary_path() -> Result<std::path::PathBuf, String> {
    explicit_arguments().map(|(path, _)| path)
}
/// Remaining binary arguments after validating the one explicit vocabulary input.
/// The executable name is excluded, matching `std::env::args().skip(1)`.
pub fn qualification_args() -> Result<Vec<String>, String> {
    let (path, remaining) = explicit_arguments()?;
    load_vocabulary(&path)?;
    Ok(remaining)
}
/// Qualification callers must explicitly supply their source-selected vocabulary.
///
/// # Errors
/// Returns an error for missing/conflicting inputs, unreadable files, or invalid vocabulary data.
pub fn selected_vocabulary() -> Result<std::sync::Arc<CreatureTypeVocabulary>, String> {
    let (path, _) = explicit_arguments()?;
    load_vocabulary(&path)
}
/// Clearly synthetic vocabulary for independent native unit fixtures only.
pub fn synthetic_vocabulary() -> std::sync::Arc<CreatureTypeVocabulary> {
    synthetic_vocabulary_for_rules("qualification-synthetic")
}
/// Synthetic test source with an explicit rules label; never publication evidence.
pub fn synthetic_vocabulary_for_rules(rules: &str) -> std::sync::Arc<CreatureTypeVocabulary> {
    std::sync::Arc::new(
        CreatureTypeVocabulary::new(rules.into(), [0; 32], vec!["Cat".into(), "Elf".into()])
            .expect("synthetic fixture vocabulary"),
    )
}

#[cfg(test)]
mod tests {
    use super::super::packet::unique_json;
    use super::*;
    use serde_json::json;

    fn vocabulary() -> CreatureTypeVocabulary {
        CreatureTypeVocabulary::new("CR-test".into(), [1; 32], vec!["Cat".into(), "Elf".into()])
            .unwrap()
    }

    #[test]
    fn ids_require_canonical_syntax_and_actual_membership() {
        let vocabulary = vocabulary();
        let valid = json!({"vocabulary":vocabulary.id().to_string(),"ordinal":"1"});
        assert_eq!(
            id(valid.clone(), &vocabulary).unwrap(),
            vocabulary.resolve_label("Elf").unwrap()
        );
        for ordinal in [
            json!(1),
            json!("01"),
            json!("+1"),
            json!("65536"),
            json!("2"),
        ] {
            let mut value = valid.clone();
            value["ordinal"] = ordinal;
            assert!(id(value, &vocabulary).is_err());
        }
        assert!(id(json!({"tag":"CardName","value":"Elf"}), &vocabulary).is_err());
        assert!(
            id(
                json!({"vocabulary":"0".repeat(64),"ordinal":"0"}),
                &vocabulary
            )
            .is_err()
        );
        assert!(unique_json(br#"{"vocabulary":"x","ordinal":"0","ordinal":"1"}"#).is_err());
    }

    #[test]
    fn slots_do_not_accept_numeric_or_noncanonical_inputs() {
        assert_eq!(slot(json!("65535")).unwrap().ordinal(), 65535);
        for value in [
            json!(0),
            json!("-0"),
            json!("00"),
            json!("65536"),
            json!(" 1"),
        ] {
            assert!(slot(value).is_err());
        }
    }
}
