//! Qualified pack labels and purchase/valuation quotes without private recipe evidence.
use crate::{
    store::validate_page_ids,
    supply::{PackQuantity, PackQuote},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bound labels to a small independently paged catalogue response.
pub const PACK_PRODUCT_PAGE_LIMIT: usize = 32;
/// One qualified recipe identity; labels do not certify complete game-engine support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackProduct {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub product_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub set_id: Uuid,
    pub name: String,
    pub set_name: String,
    pub revision_ref: String,
}
/// Caller-bound labels; a full terminal page may conservatively lead to an empty page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackProductPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<PackProduct>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl PackProductPage {
    /// Validate cursor progression, exact page bounds and public label identities.
    ///
    /// # Errors
    /// Rejects invalid IDs, duplicate product identities, malformed labels or unsolicited pages.
    pub fn validate(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        if self.account_id.is_nil() || !(1..=PACK_PRODUCT_PAGE_LIMIT).contains(&limit) {
            return Err("invalid pack catalogue scope");
        }
        validate_page_ids(&self.items, self.next_cursor, after, limit, |p| p.id)?;
        let mut products = std::collections::BTreeSet::new();
        for item in &self.items {
            if item.product_id.is_nil()
                || item.set_id.is_nil()
                || !products.insert(item.product_id)
                || [&item.name, &item.set_name, &item.revision_ref]
                    .iter()
                    .any(|v| {
                        v.is_empty() || v.chars().count() > 1024 || v.chars().any(char::is_control)
                    })
            {
                return Err("invalid pack product labels");
            }
        }
        Ok(())
    }
}
/// Current aggregate sequential pricing; it is an observation, never a stock reservation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedPackQuote {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub quote: PackQuote,
}
impl OwnedPackQuote {
    /// Bind the root owner and the original recipe/quantity request.
    ///
    /// # Errors
    /// Rejects nil ownership and mismatched quote identity or quantity.
    pub fn validate(&self, recipe: Uuid, quantity: PackQuantity) -> Result<(), &'static str> {
        if self.account_id.is_nil() {
            return Err("invalid pack quote owner");
        }
        self.quote.validate_request(recipe, quantity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn shop_contracts_bind_pages_requests_and_required_nulls() {
        let f: Value =
            serde_json::from_str(include_str!("../tests/fixtures/pack-shop.json")).unwrap();
        let page: PackProductPage = serde_json::from_value(f["products"].clone()).unwrap();
        page.validate(None, 32).unwrap();
        assert!(page.validate(Some(page.items[0].id), 32).is_err());
        for key in ["purchase", "valuation"] {
            let quote: OwnedPackQuote = serde_json::from_value(f[key].clone()).unwrap();
            let quantity = serde_json::from_value(f[key]["quote"]["quantity"].clone()).unwrap();
            quote.validate(page.items[0].id, quantity).unwrap();
            assert!(quote.validate(Uuid::new_v4(), quantity).is_err());
        }
        let mut bad = f["valuation"].clone();
        bad["quote"].as_object_mut().unwrap().remove("total");
        assert!(serde_json::from_value::<OwnedPackQuote>(bad).is_err());
        let mut bad = f["purchase"].clone();
        bad["quote"]["recipeId"] = json!(Uuid::nil());
        assert!(serde_json::from_value::<OwnedPackQuote>(bad).is_err());
        let mut bad = f["products"].clone();
        bad.as_object_mut().unwrap().remove("nextCursor");
        assert!(serde_json::from_value::<PackProductPage>(bad).is_err());
    }
}
