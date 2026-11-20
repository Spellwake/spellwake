//! Player prediction commands contain terms only; session identity and market outcomes are authoritative.

use crate::amount::Amount;
use serde::{Deserialize, Deserializer, Serialize, de};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum PredictionSide {
    Yes,
    No,
}
impl TryFrom<String> for PredictionSide {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            _ => Err("invalid string enum value"),
        }
    }
}

/// A whole-coin maximum cost per fixed 100-coin contract, encoded as a decimal string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ContractBid(Amount);

impl ContractBid {
    pub fn amount(&self) -> &Amount {
        &self.0
    }
}
impl TryFrom<Amount> for ContractBid {
    type Error = &'static str;

    fn try_from(amount: Amount) -> Result<Self, Self::Error> {
        if amount.is_zero() || amount > Amount::from(99) {
            return Err("contract bid must be between 1 and 99 whole coins");
        }
        Ok(Self(amount))
    }
}
impl<'de> Deserialize<'de> for ContractBid {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(Amount::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlacePredictionOrder {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub market_id: Uuid,
    pub side: PredictionSide,
    pub limit_cost: ContractBid,
    pub quantity: Amount,
}
impl PlacePredictionOrder {
    /// Check aggregate precision before invoking the same database-enforced business rule.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.quantity.is_zero() {
            return Err("prediction quantity must be positive");
        }
        Amount::from_biguint(self.quantity.integer() * self.limit_cost.amount().integer())
            .map_err(|_| "prediction reservation exceeds supported precision")?;
        Ok(())
    }
}

/// Only unfilled units can be canceled; filled positions have no resale command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelPredictionOrder {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub order_id: Uuid,
}

pub(crate) fn price_decimal(value: &str, positive: bool) -> bool {
    if value.len() > 80 {
        return false;
    }
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or("");
    let fraction = parts.next();
    !whole.is_empty()
        && (whole == "0" || !whole.starts_with('0'))
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        && parts.next().is_none()
        && (!positive || value.bytes().any(|b| matches!(b, b'1'..=b'9')))
}
/// Sampled condition chosen in immutable market terms.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum PredictionCondition {
    ReachedBy,
    AtDeadline,
}
impl TryFrom<String> for PredictionCondition {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "reached_by" => Ok(Self::ReachedBy),
            "at_deadline" => Ok(Self::AtDeadline),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Immutable sampled-price publication; source qualification and future cutoff stay authoritative in SQL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishPrediction {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub printing_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub probe_id: Uuid,
    pub target_usd: String,
    pub condition: PredictionCondition,
    pub trading_closes_at: chrono::DateTime<chrono::Utc>,
    pub observation_starts_at: chrono::DateTime<chrono::Utc>,
    pub deadline: chrono::DateTime<chrono::Utc>,
}
impl PublishPrediction {
    /// Validate shared wire bounds and microsecond timestamp ordering before SQL evaluates current time/source.
    ///
    /// # Errors
    /// Rejects malformed positive USD targets, unsupported instant precision and invalid term ranges.
    pub fn validate(&self) -> Result<(), &'static str> {
        use chrono::Datelike;
        if !price_decimal(&self.target_usd, true)
            || [
                self.trading_closes_at,
                self.observation_starts_at,
                self.deadline,
            ]
            .iter()
            .any(|v| {
                !(1..=9999).contains(&v.year())
                    || v.timestamp_subsec_nanos() >= 1_000_000_000
                    || v.timestamp_subsec_nanos() % 1000 != 0
            })
            || self.trading_closes_at >= self.observation_starts_at
            || self.observation_starts_at > self.deadline
            || self.deadline - self.observation_starts_at > chrono::Duration::days(366)
        {
            return Err("invalid publication terms");
        }
        Ok(())
    }
}

/// Authenticated intent over canonical matching; no actor, hold or outcome is accepted.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PredictionCommand {
    PublishMarket(PublishPrediction),
    PlaceOrder(PlacePredictionOrder),
    CancelOrder(CancelPredictionOrder),
}
impl<'de> Deserialize<'de> for PredictionCommand {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let wire: crate::provider_json::CommandObject = crate::provider_json::object(input)?;
        match wire.kind.as_str() {
            "publish_market" => wire.payload().map(Self::PublishMarket),
            "place_order" => wire.payload().map(Self::PlaceOrder),
            "cancel_order" => wire.payload().map(Self::CancelOrder),
            _ => Err(serde::de::Error::custom("invalid command kind")),
        }
    }
}

impl PredictionCommand {
    /// Validate public request identities and exact reservation precision.
    ///
    /// # Errors
    /// Rejects nil request/target identity or invalid whole-contract terms.
    pub fn validate(&self) -> Result<(), &'static str> {
        let ids = match self {
            Self::PublishMarket(command) => {
                command.validate()?;
                if command.probe_id.is_nil() {
                    return Err("prediction price probe required");
                }
                [command.request_id, command.printing_id]
            }
            Self::PlaceOrder(command) => {
                command.validate()?;
                [command.request_id, command.market_id]
            }
            Self::CancelOrder(command) => [command.request_id, command.order_id],
        };
        if ids.iter().any(Uuid::is_nil) {
            return Err("prediction request and target required");
        }
        Ok(())
    }
    pub(crate) fn valid_ack(&self, ack: &crate::commands::CommandAck) -> bool {
        !ack.resource_id.is_nil()
            && match self {
                Self::CancelOrder(command) => ack.resource_id == command.order_id,
                _ => true,
            }
    }
    pub(crate) fn failure(&self, errors: &std::collections::BTreeMap<String, String>) -> bool {
        let Some((field, reason)) = errors.iter().next().filter(|_| errors.len() == 1) else {
            return false;
        };
        if (field.as_str(), reason.as_str()) == ("account", "account unavailable") {
            return true;
        }
        match self {
            Self::PublishMarket(_) => matches!(
                (field.as_str(), reason.as_str()),
                (
                    "terms",
                    "invalid future sampled-price terms"
                        | "at least one complete daily acceptance window required"
                ) | ("printing", "printing unavailable")
                    | (
                        "price",
                        "qualified printing price unavailable"
                            | "usable current exact-printing price required"
                    )
            ),
            Self::PlaceOrder(_) => matches!(
                (field.as_str(), reason.as_str()),
                ("order", "invalid whole-contract bid")
                    | (
                        "market",
                        "prediction market unavailable"
                            | "prediction trading is closed"
                            | "prediction order capacity reached"
                    )
                    | ("quantity", "reservation exceeds supported precision")
                    | ("coins", "not enough available coins")
            ),
            Self::CancelOrder(_) => matches!(
                (field.as_str(), reason.as_str()),
                (
                    "order",
                    "prediction order unavailable" | "no unfilled units to cancel"
                )
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn orders_reject_rounded_bid_values_claimed_actors_and_unbackable_quantities() {
        let value = json!({"requestId":Uuid::new_v4(),"marketId":Uuid::new_v4(),"side":"yes","limitCost":"60","quantity":"3"});
        let order: PlacePredictionOrder = serde_json::from_value(value.clone()).unwrap();
        order.validate().unwrap();
        assert_eq!(serde_json::to_value(order).unwrap(), value);
        for invalid in [
            json!(60),
            json!("060"),
            json!("60.0"),
            json!("0"),
            json!("100"),
        ] {
            let mut changed = value.clone();
            changed["limitCost"] = invalid;
            assert!(serde_json::from_value::<PlacePredictionOrder>(changed).is_err());
        }
        for quantity in ["0".to_owned(), "9".repeat(78)] {
            let mut changed = value.clone();
            changed["quantity"] = json!(quantity);
            assert!(
                serde_json::from_value::<PlacePredictionOrder>(changed)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        for key in [
            "accountId",
            "actorId",
            "outcome",
            "actualCost",
            "acceptedSequence",
        ] {
            let mut changed = value.clone();
            changed[key] = json!("untrusted");
            assert!(serde_json::from_value::<PlacePredictionOrder>(changed).is_err());
        }
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<PredictionSide>(&["yes", "no"]);
        crate::test_http::string_enum_roundtrip::<PredictionCondition>(&[
            "reached_by",
            "at_deadline",
        ]);
    }
}
