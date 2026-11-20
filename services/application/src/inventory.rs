//! Owner-scoped collection projection of the one hold-aware inventory.
use crate::amount::Amount;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AssetIdentity {
    Coin {
        #[serde(rename = "currencyKey")]
        currency_key: String,
    },
    Card {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "printingId")]
        printing_id: Uuid,
    },
    Pack {
        #[serde(deserialize_with = "crate::identifiers::uuid")]
        #[serde(rename = "recipeId")]
        recipe_id: Uuid,
    },
}
impl AssetIdentity {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Coin { currency_key } if currency_key.is_empty() => {
                Err("currency identity required")
            }
            Self::Card { printing_id } if printing_id.is_nil() => Err("printing identity required"),
            Self::Pack { recipe_id } if recipe_id.is_nil() => Err("recipe identity required"),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryItem {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub asset_id: Uuid,
    pub revision: Amount,
    pub owned: Amount,
    pub held: Amount,
    pub available: Amount,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub asset: AssetIdentity,
}
impl InventoryItem {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil() || self.asset_id.is_nil() || self.owned.is_zero() {
            return Err("invalid owned inventory identity or quantity");
        }
        if self.owned.checked_sub(&self.held).ok().as_ref() != Some(&self.available) {
            return Err("inconsistent owned, held and available quantities");
        }
        self.asset.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryPage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<InventoryItem>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl InventoryPage {
    /// Reject an internally valid page that replays rows before the requested cursor.
    pub fn validate_after(&self, limit: usize, after: Option<Uuid>) -> Result<(), &'static str> {
        self.validate(limit)?;
        if after.is_some_and(|cursor| self.items.iter().any(|item| item.id <= cursor)) {
            return Err("inventory precedes requested cursor");
        }
        Ok(())
    }

    /// A full final page may return its last ID, followed by an empty page.
    /// Paging orders identities; it does not freeze balances across requests.
    pub fn validate(&self, limit: usize) -> Result<(), &'static str> {
        if !(1..=100).contains(&limit) || self.items.len() > limit {
            return Err("invalid inventory page bounds");
        }
        for item in &self.items {
            item.validate()?;
        }
        if self.items.windows(2).any(|pair| pair[0].id >= pair[1].id) {
            return Err("inventory page is not strictly ordered");
        }
        let expected = if self.items.len() == limit {
            self.items.last().map(|item| item.id)
        } else {
            None
        };
        if self.next_cursor != expected {
            return Err("inventory cursor differs from page contents");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collection_counts_and_asset_identities_are_exact_and_consistent() {
        let item = json!({"id":Uuid::new_v4(),"assetId":Uuid::new_v4(),"revision":"2","owned":"9007199254740993","held":"1","available":"9007199254740992","asset":{"kind":"pack","recipeId":Uuid::new_v4()}});
        serde_json::from_value::<InventoryItem>(item.clone())
            .unwrap()
            .validate()
            .unwrap();
        for (field, value) in [
            ("held", json!("9007199254740994")),
            ("available", json!("9007199254740991")),
            ("owned", json!("0")),
        ] {
            let mut bad = item.clone();
            bad[field] = value;
            assert!(
                serde_json::from_value::<InventoryItem>(bad)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        let mut bad = item.clone();
        bad["owned"] = json!(9007199254740993u64);
        assert!(serde_json::from_value::<InventoryItem>(bad).is_err());
        let mut bad = item;
        bad["asset"]["printingId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<InventoryItem>(bad).is_err());
    }
}
