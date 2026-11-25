//! Caller-only saved deck content, separate from card search and legality.
use super::{CustomDeckEntry, DeckEntry, validate_all_entries};
use crate::{
    account::{DeckSummary, FormatSummary},
    amount::Amount,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedRevision {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub number: Amount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub entries: Vec<DeckEntry>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "crate::provider_json::objects"
    )]
    pub custom_entries: Vec<CustomDeckEntry>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeckDetail {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub deck: DeckSummary,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub revision: Option<SavedRevision>,
}
impl DeckDetail {
    /// Check requested identity and a consistent current content revision.
    ///
    /// # Errors
    /// Returns a static error for inconsistent pointers or malformed content.
    pub fn validate(&self, requested: Uuid) -> Result<(), &'static str> {
        self.deck.validate()?;
        if self.deck.id != requested
            || self.deck.current_revision_id != self.revision.as_ref().map(|revision| revision.id)
        {
            return Err("inconsistent saved deck revision");
        }
        if let Some(revision) = &self.revision {
            if revision.id.is_nil() || revision.number.is_zero() {
                return Err("invalid saved revision identity");
            }
            validate_all_entries(&revision.entries, &revision.custom_entries)?;
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatPage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<FormatSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl FormatPage {
    /// Validate exact bounded catalogue pagination.
    ///
    /// # Errors
    /// Returns a static error for invalid ordering, identities or cursors.
    pub fn validate(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        if !(1..=100).contains(&limit)
            || self.items.len() > limit
            || self.items.iter().any(|item| {
                item.id.is_nil()
                    || item.key.is_empty()
                    || item.name.is_empty()
                    || after.is_some_and(|id| item.id <= id)
            })
            || self.items.windows(2).any(|pair| pair[0].id >= pair[1].id)
            || self.next_cursor
                != if self.items.len() == limit {
                    self.items.last().map(|item| item.id)
                } else {
                    None
                }
        {
            return Err("invalid format catalogue page");
        }
        Ok(())
    }
}
