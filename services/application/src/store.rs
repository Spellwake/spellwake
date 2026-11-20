//! Caller-visible catalogue metadata and purchase recovery, without provider identifiers.
use crate::stripe::PurchaseView;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum CosmeticKind {
    Back,
    Sleeve,
    Dice,
    Playmat,
}
impl TryFrom<String> for CosmeticKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "back" => Ok(Self::Back),
            "sleeve" => Ok(Self::Sleeve),
            "dice" => Ok(Self::Dice),
            "playmat" => Ok(Self::Playmat),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CosmeticSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub name: String,
    pub kind: CosmeticKind,
    pub is_free: bool,
    pub is_owned: bool,
    pub included_with_patreon: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct StorePage<T> {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<T>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}

impl<T> StorePage<T> {
    pub(crate) fn validate_ids(
        &self,
        after: Option<Uuid>,
        limit: usize,
        id: impl Fn(&T) -> Uuid,
    ) -> Result<(), &'static str> {
        validate_page_ids(&self.items, self.next_cursor, after, limit, id)
    }
}
pub(crate) fn validate_page_ids<T>(
    items: &[T],
    next_cursor: Option<Uuid>,
    after: Option<Uuid>,
    limit: usize,
    id: impl Fn(&T) -> Uuid,
) -> Result<(), &'static str> {
    if !(1..=100).contains(&limit)
        || items.len() > limit
        || items
            .iter()
            .any(|item| id(item).is_nil() || after.is_some_and(|cursor| id(item) <= cursor))
        || items.windows(2).any(|pair| id(&pair[0]) >= id(&pair[1]))
        || next_cursor
            != if items.len() == limit {
                items.last().map(id)
            } else {
                None
            }
    {
        return Err("invalid store page");
    }
    Ok(())
}
impl StorePage<CosmeticSummary> {
    /// Validate bounded, ordered catalogue metadata from the caller's RLS projection.
    ///
    /// # Errors
    /// Returns a static contract error for malformed names, ownership or pagination.
    pub fn validate_after(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        self.validate_ids(after, limit, |item| item.id)?;
        if self.items.iter().any(|item| {
            !(1..=120).contains(&item.name.chars().count())
                || item.name.trim_matches(' ') != item.name
                || (item.is_free && !item.is_owned)
        }) {
            return Err("invalid cosmetic metadata");
        }
        Ok(())
    }
}
impl StorePage<PurchaseView> {
    /// Validate owner purchase recovery without exposing private payment references.
    ///
    /// # Errors
    /// Returns a static contract error for malformed purchases or pagination.
    pub fn validate_after(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        self.validate_ids(after, limit, |item| item.purchase_id)?;
        for item in &self.items {
            item.validate(item.purchase_id)
                .map_err(|_| "invalid purchase")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<CosmeticKind>(&[
            "back", "sleeve", "dice", "playmat",
        ]);
    }
}
