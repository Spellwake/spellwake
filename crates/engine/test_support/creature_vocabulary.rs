//! Engine-independent strict vocabulary input boundary shared with qualification CLIs.
use serde::Deserialize;
use serde_json::Value;
use spellwake_contracts::creature_types::{
    CreatureTypeId, CreatureTypeVocabulary, CreatureVocabularyId,
};

/// Full immutable vocabulary observation shared by every qualification owner.
pub fn view(value: &CreatureTypeVocabulary) -> Value {
    let rules_sha256 = value
        .rules_sha256()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    serde_json::json!({"rules_revision":value.rules_revision(),"rules_sha256":rules_sha256,"members":value.members(),"id":value.id().to_string()})
}

pub fn member_view(value: CreatureTypeId) -> Value {
    serde_json::json!({"vocabulary":value.vocabulary().to_string(),"ordinal":value.ordinal().to_string()})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VocabularyInput {
    rules_revision: String,
    rules_sha256: String,
    members: Vec<String>,
    id: String,
}
/// Decode one explicitly supplied vocabulary, checking its declared identity.
pub fn vocabulary(value: Value) -> Result<CreatureTypeVocabulary, String> {
    let raw: VocabularyInput = serde_json::from_value(value).map_err(|error| error.to_string())?;
    construct(raw)
}
fn construct(raw: VocabularyInput) -> Result<CreatureTypeVocabulary, String> {
    let hash =
        CreatureVocabularyId::try_from(raw.rules_sha256).map_err(|error| format!("{error:?}"))?;
    let claimed = CreatureVocabularyId::try_from(raw.id).map_err(|error| format!("{error:?}"))?;
    let vocabulary = CreatureTypeVocabulary::new(raw.rules_revision, *hash.as_bytes(), raw.members)
        .map_err(|error| format!("{error:?}"))?;
    if vocabulary.id() != claimed {
        return Err("declared creature vocabulary identity does not match its actual value".into());
    }
    Ok(vocabulary)
}
/// Read the exact flat vocabulary DTO; serde rejects repeated or unknown fields.
pub fn load_vocabulary(
    path: &std::path::Path,
) -> Result<std::sync::Arc<CreatureTypeVocabulary>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let raw: VocabularyInput = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    construct(raw).map(std::sync::Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_flat_input_rejects_unknown_duplicate_missing_and_false_identity_fields() {
        let native =
            CreatureTypeVocabulary::new("synthetic-codec".into(), [0; 32], vec!["Cat".into()])
                .unwrap();
        let raw = serde_json::json!({"rules_revision":native.rules_revision(),"rules_sha256":"0".repeat(64),"members":native.members(),"id":native.id().to_string()});
        assert_eq!(view(&native), raw);
        assert_eq!(
            member_view(native.resolve_label("Cat").unwrap()),
            serde_json::json!({"vocabulary":native.id().to_string(),"ordinal":"0"})
        );
        assert_eq!(vocabulary(raw.clone()).unwrap(), native);
        let mut wrong = raw.clone();
        wrong["id"] = serde_json::json!("0".repeat(64));
        assert!(vocabulary(wrong).is_err());
        let mut missing = raw.clone();
        missing.as_object_mut().unwrap().remove("id");
        assert!(vocabulary(missing).is_err());
        let mut extra = raw;
        extra["default"] = serde_json::json!(true);
        assert!(vocabulary(extra).is_err());
        assert!(serde_json::from_str::<VocabularyInput>(r#"{"rules_revision":"x","rules_revision":"y","rules_sha256":"x","members":[],"id":"x"}"#).is_err());
    }
}
