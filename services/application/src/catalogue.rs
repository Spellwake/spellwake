//! Public card labels and exact printing metadata, without artwork or gameplay claims.
use crate::store::StorePage;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CardSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub name: String,
    pub oracle_ref: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub name: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub code: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrintingSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub definition_id: Uuid,
    pub external_printing_ref: String,
    pub rarity: String,
    pub finish: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub collector_number: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub language: Option<String>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub set: SetSummary,
}
fn text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.chars().count() <= maximum && !value.contains('\0')
}
fn reference(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && value.bytes().all(|byte| byte.is_ascii_graphic())
}
impl CardSummary {
    /// Validate display metadata without asserting executability or format legality.
    ///
    /// # Errors
    /// Returns a static error for malformed identifiers or bounded text.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil() || !text(&self.name, 1024) || !reference(&self.oracle_ref, 128) {
            return Err("invalid card metadata");
        }
        Ok(())
    }
}
impl SetSummary {
    /// Validate public set identity without asserting purchasability or game support.
    ///
    /// # Errors
    /// Rejects nil identity, malformed labels and invalid optional set codes.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil()
            || !text(&self.name, 1024)
            || self.code.as_ref().is_some_and(|value| {
                value.is_empty()
                    || value.len() > 16
                    || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        {
            return Err("invalid set metadata");
        }
        Ok(())
    }
}
impl PrintingSummary {
    /// Validate one finish-specific identity and preserve unknown optional labels.
    ///
    /// # Errors
    /// Returns a static error for malformed identities, references or labels.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.set.validate()?;
        if self.id.is_nil()
            || self.definition_id.is_nil()
            || !reference(&self.external_printing_ref, 128)
            || !reference(&self.rarity, 64)
            || !reference(&self.finish, 64)
            || self
                .collector_number
                .as_ref()
                .is_some_and(|value| !text(value, 128))
            || self.language.as_ref().is_some_and(|value| {
                value.is_empty()
                    || value.len() > 16
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
        {
            return Err("invalid printing metadata");
        }
        Ok(())
    }
}
pub type SetPage = StorePage<SetSummary>;
impl StorePage<SetSummary> {
    /// Validate bounded ordered set discovery pages.
    ///
    /// # Errors
    /// Rejects malformed labels, identities, ordering and cursor progression.
    pub fn validate_after(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        self.validate_ids(after, limit, |item| item.id)?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}
pub type CardPage = StorePage<CardSummary>;
pub type PrintingPage = StorePage<PrintingSummary>;
impl StorePage<CardSummary> {
    /// Validate bounded deterministic card pages.
    ///
    /// # Errors
    /// Returns a static error for invalid ordering, pagination or labels.
    pub fn validate_after(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        self.validate_ids(after, limit, |item| item.id)?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}
impl StorePage<PrintingSummary> {
    /// Validate pagination and that every printing belongs to the requested definition.
    ///
    /// # Errors
    /// Returns a static error for malformed metadata, mismatched definitions or cursors.
    pub fn validate_after(
        &self,
        definition: Option<Uuid>,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        self.validate_ids(after, limit, |item| item.id)?;
        for item in &self.items {
            item.validate()?;
            if definition.is_some_and(|definition| item.definition_id != definition) {
                return Err("printing belongs to another definition");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogueLookup {
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub definition_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub printing_ids: Vec<Uuid>,
}
/// One printing selector plus optional source-specific set/collector hints.
pub(crate) struct PrintingFilter<'a> {
    pub definition: Option<Uuid>,
    pub name: Option<&'a str>,
    pub set_code: Option<&'a str>,
    pub collector: Option<&'a str>,
}
impl PrintingFilter<'_> {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.definition.is_some() == self.name.is_some()
            || self.definition.is_some_and(|id| id.is_nil())
            || self.name.is_some_and(|value| !text(value, 1024))
            || self.set_code.is_some_and(|value| {
                value.is_empty()
                    || value.len() > 16
                    || !value.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
            || self.collector.is_some_and(|value| !text(value, 128))
        {
            return Err("invalid printing selector");
        }
        Ok(())
    }
}
impl CatalogueLookup {
    /// Bound batch lookups and reject duplicate or nil identities.
    ///
    /// # Errors
    /// Returns a static error when either list violates the100-item contract.
    pub fn validate(&self) -> Result<(), &'static str> {
        for ids in [&self.definition_ids, &self.printing_ids] {
            if ids.len() > 100
                || ids.iter().any(Uuid::is_nil)
                || ids.iter().collect::<HashSet<_>>().len() != ids.len()
            {
                return Err("invalid catalogue lookup");
            }
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogueLabels {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub cards: Vec<CardSummary>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub printings: Vec<PrintingSummary>,
}
impl CatalogueLabels {
    /// Confirm that only requested identities were returned; absence remains explicit.
    ///
    /// # Errors
    /// Returns a static error for malformed labels, duplicates or unsolicited identities.
    pub fn validate(&self, request: &CatalogueLookup) -> Result<(), &'static str> {
        request.validate()?;
        if self.cards.len() > request.definition_ids.len()
            || self.printings.len() > request.printing_ids.len()
            || self.cards.windows(2).any(|pair| pair[0].id >= pair[1].id)
            || self
                .printings
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
        {
            return Err("invalid catalogue labels");
        }
        for item in &self.cards {
            item.validate()?;
            if !request.definition_ids.contains(&item.id) {
                return Err("unsolicited card identity");
            }
        }
        for item in &self.printings {
            item.validate()?;
            if !request.printing_ids.contains(&item.id) {
                return Err("unsolicited printing identity");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_bounded_label_batch_fits_the_shared_response_limit() {
        let mut cards = Vec::new();
        let mut printings = Vec::new();
        for _ in 0..100 {
            cards.push(CardSummary {
                id: Uuid::new_v4(),
                name: "💠".repeat(1024),
                oracle_ref: "\\".repeat(128),
            });
            printings.push(PrintingSummary {
                id: Uuid::new_v4(),
                definition_id: Uuid::new_v4(),
                external_printing_ref: "\\".repeat(128),
                rarity: "\\".repeat(64),
                finish: "\\".repeat(64),
                collector_number: Some("💠".repeat(128)),
                language: Some("a".repeat(16)),
                set: SetSummary {
                    id: Uuid::new_v4(),
                    name: "💠".repeat(1024),
                    code: Some("a".repeat(16)),
                },
            });
        }
        cards.sort_by_key(|card| card.id);
        printings.sort_by_key(|printing| printing.id);
        let request = CatalogueLookup {
            definition_ids: cards.iter().map(|card| card.id).collect(),
            printing_ids: printings.iter().map(|printing| printing.id).collect(),
        };
        let labels = CatalogueLabels { cards, printings };
        labels.validate(&request).unwrap();
        let bytes = serde_json::to_vec(&crate::ApiResponse::Success { data: labels }).unwrap();
        // Leave space for SQL JSON whitespace as well as the compact API encoding.
        assert!(
            bytes.len() + 16384 <= 1024 * 1024,
            "{} bytes exceed the shared response budget",
            bytes.len()
        );
    }

    #[test]
    fn labels_reject_unrequested_identities_and_preserve_missing_requested_records() {
        let wanted = Uuid::new_v4();
        let request = CatalogueLookup {
            definition_ids: vec![wanted],
            printing_ids: vec![],
        };
        let mut labels = CatalogueLabels {
            cards: vec![],
            printings: vec![],
        };
        assert!(labels.validate(&request).is_ok());
        labels.cards.push(CardSummary {
            id: Uuid::new_v4(),
            name: "Label".into(),
            oracle_ref: "test:source".into(),
        });
        assert!(labels.validate(&request).is_err());
        assert!(
            CatalogueLookup {
                definition_ids: vec![wanted, wanted],
                printing_ids: vec![]
            }
            .validate()
            .is_err()
        );
    }
}
