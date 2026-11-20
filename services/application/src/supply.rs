//! Supply wire inputs carry a quote identity, never an authoritative price or stock claim.

use crate::amount::Amount;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Amount", into = "Amount")]
pub struct PackQuantity(u16);

impl PackQuantity {
    pub fn get(self) -> u16 {
        self.0
    }
}

impl TryFrom<Amount> for PackQuantity {
    type Error = &'static str;

    fn try_from(value: Amount) -> Result<Self, Self::Error> {
        let quantity: u16 = value
            .to_string()
            .parse()
            .map_err(|_| "invalid pack quantity")?;
        if !(1..=1000).contains(&quantity) {
            return Err("pack quantity must be between 1 and 1000");
        }
        Ok(Self(quantity))
    }
}

impl From<PackQuantity> for Amount {
    fn from(value: PackQuantity) -> Self {
        u64::from(value.0).into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuyPacks {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub recipe_id: Uuid,
    pub quantity: PackQuantity,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub expected_policy_id: Uuid,
    pub expected_revision: Amount,
    pub expected_total: Amount,
}

impl BuyPacks {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.expected_total.is_zero() {
            return Err("purchase total must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum QuotePurpose {
    Purchase,
    ValuationOnly,
}
impl TryFrom<String> for QuotePurpose {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "purchase" => Ok(Self::Purchase),
            "valuation_only" => Ok(Self::ValuationOnly),
            _ => Err("invalid string enum value"),
        }
    }
}

/// A valuation-only quote never authorizes a purchase, including exhausted stock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "WireQuote", into = "WireQuote")]
pub struct PackQuote {
    recipe_id: Uuid,
    product_id: Uuid,
    policy_id: Uuid,
    revision: Amount,
    remaining: Amount,
    unit_price: Amount,
    quantity: PackQuantity,
    total: Option<Amount>,
    purpose: QuotePurpose,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireQuote {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    recipe_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    product_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    policy_id: Uuid,
    revision: Amount,
    remaining: Amount,
    unit_price: Amount,
    quantity: PackQuantity,
    #[serde(deserialize_with = "required_total")]
    total: Option<Amount>,
    purpose: QuotePurpose,
}
fn required_total<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Amount>, D::Error> {
    Option::deserialize(deserializer)
}

impl TryFrom<WireQuote> for PackQuote {
    type Error = &'static str;

    fn try_from(value: WireQuote) -> Result<Self, Self::Error> {
        let available = value.remaining >= Amount::from(value.quantity);
        let has_total = value.total.as_ref().is_some_and(|total| !total.is_zero());
        if value.recipe_id.is_nil()
            || value.product_id.is_nil()
            || value.policy_id.is_nil()
            || value.unit_price.is_zero()
            || match value.purpose {
                QuotePurpose::Purchase => !available || !has_total,
                QuotePurpose::ValuationOnly => available || value.total.is_some(),
            }
        {
            return Err("quote purpose and available purchase terms do not agree");
        }
        Ok(Self {
            recipe_id: value.recipe_id,
            product_id: value.product_id,
            policy_id: value.policy_id,
            revision: value.revision,
            remaining: value.remaining,
            unit_price: value.unit_price,
            quantity: value.quantity,
            total: value.total,
            purpose: value.purpose,
        })
    }
}

impl PackQuote {
    /// Reject a quote for another recipe or requested quantity.
    ///
    /// # Errors
    /// Returns a static error when the requested identity/quantity differs.
    pub fn validate_request(
        &self,
        recipe: Uuid,
        quantity: PackQuantity,
    ) -> Result<(), &'static str> {
        if self.recipe_id != recipe || self.quantity != quantity {
            return Err("quote request mismatch");
        }
        Ok(())
    }
}

impl From<PackQuote> for WireQuote {
    fn from(value: PackQuote) -> Self {
        Self {
            recipe_id: value.recipe_id,
            product_id: value.product_id,
            policy_id: value.policy_id,
            revision: value.revision,
            remaining: value.remaining,
            unit_price: value.unit_price,
            quantity: value.quantity,
            total: value.total,
            purpose: value.purpose,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pack_quantity_is_an_exact_bounded_decimal_string() {
        for invalid in [
            json!(1),
            json!("0"),
            json!("1001"),
            json!("01"),
            json!("1.0"),
        ] {
            assert!(serde_json::from_value::<PackQuantity>(invalid).is_err());
        }
        let mut request = json!({"requestId":Uuid::new_v4(),"recipeId":Uuid::new_v4(),"quantity":"1","expectedPolicyId":Uuid::new_v4(),"expectedRevision":"0","expectedTotal":"0"});
        assert!(
            serde_json::from_value::<BuyPacks>(request.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        request["expectedTotal"] = json!("1000");
        assert!(
            serde_json::from_value::<BuyPacks>(request.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        request["ownerId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<BuyPacks>(request).is_err());
        assert_eq!(
            serde_json::from_value::<PackQuantity>(json!("1000"))
                .unwrap()
                .get(),
            1000
        );
    }

    #[test]
    fn sold_out_or_missing_total_cannot_deserialize_as_a_purchase() {
        let mut quote = json!({"recipeId":Uuid::new_v4(),"productId":Uuid::new_v4(),"policyId":Uuid::new_v4(),"revision":"1","remaining":"0","unitPrice":"2000","quantity":"1","total":null,"purpose":"valuation_only"});
        assert!(serde_json::from_value::<PackQuote>(quote.clone()).is_ok());
        quote["purpose"] = json!("purchase");
        assert!(serde_json::from_value::<PackQuote>(quote.clone()).is_err());
        quote["remaining"] = json!("1");
        quote["total"] = json!("1000");
        assert!(serde_json::from_value::<PackQuote>(quote.clone()).is_ok());
        quote["total"] = json!(null);
        assert!(serde_json::from_value::<PackQuote>(quote).is_err());
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<QuotePurpose>(&["purchase", "valuation_only"]);
    }
}
