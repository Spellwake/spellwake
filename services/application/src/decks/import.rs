//! Atomic resolved-card imports reuse saved deck entry semantics and receipts.
use super::{CreateDeck, DeckEntry, PlayMode, validate_entries};
use crate::commands::CommandDisposition;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// Already-resolved catalogue entries; text resolution remains a separate caller step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportDeck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    pub name: String,
    pub mode: PlayMode,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub entries: Vec<DeckEntry>,
}
impl ImportDeck {
    /// Validate metadata, exact quantities and the complete bounded intent.
    ///
    /// # Errors
    /// Rejects invalid metadata, duplicate/invalid entries and oversized payloads.
    pub fn validate(&self) -> Result<(), &'static str> {
        CreateDeck {
            request_id: self.request_id,
            name: self.name.clone(),
            mode: self.mode,
            format_id: self.format_id,
        }
        .validate()?;
        validate_entries(&self.entries)?;
        if serde_json::to_vec(self)
            .map_err(|_| "invalid deck import")?
            .len()
            > 262144
        {
            return Err("deck import is too large");
        }
        Ok(())
    }
}

/// IDs of the new library deck and its first immutable sealed revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportAck {
    pub resource_id: Uuid,
    pub revision_id: Uuid,
}
impl<'de> Deserialize<'de> for ImportAck {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            resource_id: Uuid,
            #[serde(deserialize_with = "crate::identifiers::uuid")]
            revision_id: Uuid,
        }
        let value: Wire = crate::provider_json::object(input)?;
        if value.resource_id.is_nil()
            || value.revision_id.is_nil()
            || value.resource_id == value.revision_id
        {
            return Err(serde::de::Error::custom("invalid import acknowledgement"));
        }
        Ok(Self {
            resource_id: value.resource_id,
            revision_id: value.revision_id,
        })
    }
}
/// Receipt lookup retains the import-specific two-ID success without widening other acknowledgements.
pub type ImportDisposition = CommandDisposition<ImportAck>;

/// Typed transport result disambiguates execute acknowledgements from lookup dispositions.
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum ImportResponse {
    Ack(ImportAck),
    Disposition(ImportDisposition),
}

pub(crate) fn valid_failure(errors: &std::collections::BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && matches!(
            errors.iter().next().map(|(k, v)| (k.as_str(), v.as_str())),
            Some(
                ("account", "unavailable" | "account unavailable")
                    | ("deck", "invalid deck metadata" | "deck unavailable")
                    | (
                        "entries",
                        "expected deck entry array"
                            | "too many deck entries"
                            | "invalid deck entries"
                    )
                    | ("cosmetics", "default cosmetic unavailable")
            )
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn import_contract_rejects_duplicate_entries_quantities_and_forged_authority() {
        let entry = json!({"definitionId":Uuid::new_v4(),"printingId":null,"section":"main","quantity":"9007199254740993"});
        let value = json!({"requestId":Uuid::new_v4(),"name":"Import","mode":"open","formatId":Uuid::new_v4(),"entries":[entry.clone()]});
        assert!(
            serde_json::from_value::<ImportDeck>(value.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        let mut duplicate = value.clone();
        duplicate["entries"] = json!([entry.clone(), entry]);
        assert!(
            serde_json::from_value::<ImportDeck>(duplicate)
                .unwrap()
                .validate()
                .is_err()
        );
        let mut numeric = value.clone();
        numeric["entries"][0]["quantity"] = json!(1);
        assert!(serde_json::from_value::<ImportDeck>(numeric).is_err());
        let mut authority = value;
        authority["actorId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<ImportDeck>(authority).is_err());
    }
    #[test]
    fn import_receipt_requires_exact_two_id_object_and_matching_state() {
        let id = Uuid::new_v4();
        let revision = Uuid::new_v4();
        assert!(
            serde_json::from_value::<ImportAck>(json!({"resourceId":id,"revisionId":revision}))
                .is_ok()
        );
        for value in [
            json!([id, revision]),
            json!({"resourceId":id}),
            json!({"resourceId":id,"revisionId":id}),
            json!({"resourceId":id,"revisionId":Uuid::nil()}),
        ] {
            assert!(serde_json::from_value::<ImportAck>(value).is_err());
        }
        let response = json!({"state":"accepted","response":{"status":"success","data":{"resourceId":id,"revisionId":revision}}});
        assert!(serde_json::from_value::<ImportDisposition>(response.clone()).is_ok());
        let mut invalid = response;
        invalid["state"] = json!("rejected");
        assert!(serde_json::from_value::<ImportDisposition>(invalid).is_err());
        let duplicate = format!(
            r#"{{"resourceId":"{id}","revisionId":"{revision}","revisionId":"{revision}"}}"#
        );
        assert!(serde_json::from_str::<ImportAck>(&duplicate).is_err());
    }
}
