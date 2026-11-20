//! Immutable asset terms; authority, availability and estimated values come from the server.

use crate::amount::Amount;
use serde::{Deserialize, Deserializer, Serialize, de};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetLine {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub asset_id: Uuid,
    pub quantity: Amount,
}

/// Empty bundles support incoming gifts. The complete offer must contain assets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct AssetBundle(Vec<AssetLine>);

impl AssetBundle {
    pub fn as_slice(&self) -> &[AssetLine] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl TryFrom<Vec<AssetLine>> for AssetBundle {
    type Error = &'static str;

    fn try_from(lines: Vec<AssetLine>) -> Result<Self, Self::Error> {
        if lines.len() > 100 {
            return Err("bundle exceeds 100 asset definitions");
        }
        let mut identities = HashSet::new();
        if lines
            .iter()
            .any(|line| line.quantity.is_zero() || !identities.insert(line.asset_id))
        {
            return Err("bundle requires positive quantities and distinct asset definitions");
        }
        Ok(Self(lines))
    }
}

impl<'de> Deserialize<'de> for AssetBundle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(crate::provider_json::objects(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTrade {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub recipient_id: Uuid,
    pub give: AssetBundle,
    pub request: AssetBundle,
}

impl CreateTrade {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.give.is_empty() && self.request.is_empty() {
            return Err("trade must contain at least one asset");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum TradeAction {
    Accept,
    Reject,
    Cancel,
}
impl TryFrom<String> for TradeAction {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "accept" => Ok(Self::Accept),
            "reject" => Ok(Self::Reject),
            "cancel" => Ok(Self::Cancel),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RespondTrade {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub trade_id: Uuid,
    pub action: TradeAction,
}

/// One printing or recipe-bound product, sold as a whole lot at a total price.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateListing {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub lot: AssetLine,
    pub total_price: Amount,
}

impl CreateListing {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.lot.quantity.is_zero() || self.total_price.is_zero() {
            return Err("listing quantity and total price must be positive");
        }
        Ok(())
    }
}

/// No quantity field exists: the accepted listing always transfers its entire lot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuyListing {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub listing_id: Uuid,
    pub expected_revision: Amount,
    pub expected_total: Amount,
}

impl BuyListing {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.expected_revision.is_zero() || self.expected_total.is_zero() {
            return Err("accepted listing revision and total must be positive");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelListing {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub listing_id: Uuid,
}

#[cfg(test)]
mod market_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn market_commands_cannot_partially_buy_reprice_or_claim_an_actor() {
        let mut buy = json!({"requestId":Uuid::new_v4(),"listingId":Uuid::new_v4(),"expectedRevision":"1","expectedTotal":"3000"});
        assert!(
            serde_json::from_value::<BuyListing>(buy.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        buy["quantity"] = json!("1");
        assert!(serde_json::from_value::<BuyListing>(buy.clone()).is_err());
        buy.as_object_mut().unwrap().remove("quantity");
        buy["actorId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<BuyListing>(buy).is_err());
        let listing = json!({"requestId":Uuid::new_v4(),"lot":{"assetId":Uuid::new_v4(),"quantity":"0"},"totalPrice":"10"});
        assert!(
            serde_json::from_value::<CreateListing>(listing)
                .unwrap()
                .validate()
                .is_err()
        );
        assert!(
            serde_json::from_value::<CancelListing>(
                json!({"requestId":Uuid::new_v4(),"listingId":Uuid::new_v4(),"newPrice":"10"})
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bundles_preserve_exact_terms_and_reject_duplicate_or_zero_lines() {
        let line = json!({"assetId":Uuid::new_v4(),"quantity":"999999999999999999999"});
        assert!(serde_json::from_value::<AssetBundle>(json!([line.clone()])).is_ok());
        assert!(serde_json::from_value::<AssetBundle>(json!([line.clone(), line])).is_err());
        for quantity in [json!(1), json!("0"), json!("01"), json!("1.1")] {
            assert!(
                serde_json::from_value::<AssetBundle>(
                    json!([{"assetId":Uuid::new_v4(),"quantity":quantity}])
                )
                .is_err()
            );
        }
        let too_many: Vec<_> = (0..101)
            .map(|_| json!({"assetId":Uuid::new_v4(),"quantity":"1"}))
            .collect();
        assert!(serde_json::from_value::<AssetBundle>(json!(too_many)).is_err());
    }

    #[test]
    fn offers_cannot_claim_actor_maturity_or_value_and_have_no_edit_action() {
        let mut payload =
            json!({"requestId":Uuid::new_v4(),"recipientId":Uuid::new_v4(),"give":[],"request":[]});
        assert!(
            serde_json::from_value::<CreateTrade>(payload.clone())
                .unwrap()
                .validate()
                .is_err()
        );
        payload["request"] = json!([{"assetId":Uuid::new_v4(),"quantity":"1"}]);
        assert!(
            serde_json::from_value::<CreateTrade>(payload.clone())
                .unwrap()
                .validate()
                .is_ok()
        );
        for field in ["actorId", "mature", "estimatedValue"] {
            let mut injected = payload.clone();
            injected[field] = json!("trusted");
            assert!(serde_json::from_value::<CreateTrade>(injected).is_err());
        }
        assert!(
            serde_json::from_value::<RespondTrade>(
                json!({"requestId":Uuid::new_v4(),"tradeId":Uuid::new_v4(),"action":"edit"})
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<TradeAction>(&["accept", "reject", "cancel"]);
    }
}
