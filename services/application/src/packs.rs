//! Pack intent has no caller-selected seed, recipe, contents, actor or quantity.
use crate::amount::Amount;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenPack {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub inventory_asset_id: Uuid,
}
impl OpenPack {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil() || self.inventory_asset_id.is_nil() {
            return Err("pack request and inventory identities required");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenedCard {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub printing_id: Uuid,
    pub quantity: Amount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackOutcome {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub inventory_asset_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub recipe_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub operation_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub contents: Vec<OpenedCard>,
}

/// Original-caller result envelope; opening randomness stays private.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedPackOutcome {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub outcome: PackOutcome,
}
impl OwnedPackOutcome {
    /// Validate the owner envelope and exact requested immutable receipt.
    ///
    /// # Errors
    /// Rejects nil owners, a foreign receipt or malformed outcome contents.
    pub fn validate(&self, requested: Uuid) -> Result<(), &'static str> {
        if self.account_id.is_nil() || requested.is_nil() || self.outcome.id != requested {
            return Err("invalid owned pack outcome");
        }
        self.outcome.validate()
    }
}
impl PackOutcome {
    pub fn validate(&self) -> Result<(), &'static str> {
        if [
            self.id,
            self.inventory_asset_id,
            self.recipe_id,
            self.operation_id,
        ]
        .iter()
        .any(Uuid::is_nil)
            || self.contents.is_empty()
            || self.contents.len() > 10000
        {
            return Err("invalid pack outcome identities or contents bounds");
        }
        let mut seen = BTreeSet::new();
        let mut total = Amount::from(0);
        for card in &self.contents {
            if card.printing_id.is_nil()
                || card.quantity.is_zero()
                || !seen.insert(card.printing_id)
            {
                return Err("invalid or duplicate pack output");
            }
            total = total
                .checked_add(&card.quantity)
                .map_err(|_| "pack output quantity exceeds bounds")?;
        }
        if total > Amount::from(64000) {
            return Err("pack contents exceed authored recipe bounds");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn pack_intents_cannot_choose_randomness_or_inject_contents_and_outcomes_are_exact() {
        let value = json!({"requestId":Uuid::new_v4(),"inventoryAssetId":Uuid::new_v4()});
        serde_json::from_value::<OpenPack>(value.clone())
            .unwrap()
            .validate()
            .unwrap();
        for field in ["seed", "actorId", "contents", "recipeId", "quantity"] {
            let mut injected = value.clone();
            injected[field] = json!("untrusted");
            assert!(serde_json::from_value::<OpenPack>(injected).is_err());
        }
        let mut outcome = PackOutcome {
            id: Uuid::new_v4(),
            inventory_asset_id: Uuid::new_v4(),
            recipe_id: Uuid::new_v4(),
            operation_id: Uuid::new_v4(),
            contents: vec![OpenedCard {
                printing_id: Uuid::new_v4(),
                quantity: 2.into(),
            }],
        };
        outcome.validate().unwrap();
        outcome.contents.push(outcome.contents[0].clone());
        assert!(outcome.validate().is_err());
        outcome.contents.pop();
        outcome.contents[0].quantity = 64001.into();
        assert!(outcome.validate().is_err());
        outcome.contents[0].quantity = 0.into();
        assert!(outcome.validate().is_err());
        let mut zero = value;
        zero["requestId"] = json!(Uuid::nil());
        assert!(serde_json::from_value::<OpenPack>(zero).is_err());
    }
}
