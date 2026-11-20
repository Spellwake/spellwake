//! Stored prediction terms, anonymous depth and caller-scoped exact commitments.
use crate::{
    amount::Amount,
    predictions::{ContractBid, PredictionCondition, PredictionSide},
    store::validate_page_ids,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
fn required<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

/// At most100,000 orders each below10^78 units can sum to fewer than10^83 units.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DepthQuantity(String);
impl TryFrom<String> for DepthQuantity {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > 83
            || value.is_empty()
            || value.starts_with('0')
            || !value.bytes().all(|c| c.is_ascii_digit())
        {
            return Err("invalid aggregate prediction depth");
        }
        Ok(Self(value))
    }
}
impl From<DepthQuantity> for String {
    fn from(value: DepthQuantity) -> Self {
        value.0
    }
}
/// Publication/state term labels are displayed without requalifying their source evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarketSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub printing_id: Uuid,
    pub price_metric: String,
    pub target_usd: String,
    pub condition: PredictionCondition,
    pub deadline: DateTime<Utc>,
    pub trading_closes_at: DateTime<Utc>,
    pub observation_starts_at: DateTime<Utc>,
    pub sampling_policy: String,
    pub terminal_slot: String,
    pub rules_revision: String,
    pub payout_coins: Amount,
    pub state: MarketState,
    pub accepting_orders: bool,
    #[serde(deserialize_with = "required")]
    pub outcome: Option<Outcome>,
    #[serde(deserialize_with = "required")]
    pub resolved_at: Option<DateTime<Utc>>,
}
/// Stored lifecycle; acceptingOrders separately reflects the observed trading cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum MarketState {
    Open,
    Closed,
    Resolved,
}
impl TryFrom<String> for MarketState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "open" => Ok(Self::Open),
            "closed" => Ok(Self::Closed),
            "resolved" => Ok(Self::Resolved),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Void returns original position costs rather than awarding the fixed payout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum Outcome {
    Yes,
    No,
    Void,
}
impl TryFrom<String> for Outcome {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "yes" => Ok(Self::Yes),
            "no" => Ok(Self::No),
            "void" => Ok(Self::Void),
            _ => Err("invalid string enum value"),
        }
    }
}

impl MarketSummary {
    fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil()
            || self.printing_id.is_nil()
            || !crate::predictions::price_decimal(&self.target_usd, true)
            || self.payout_coins != Amount::from(100)
            || self.trading_closes_at >= self.observation_starts_at
            || self.observation_starts_at > self.deadline
            || [
                &self.price_metric,
                &self.sampling_policy,
                &self.terminal_slot,
                &self.rules_revision,
            ]
            .iter()
            .any(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
            || (self.accepting_orders && self.state != MarketState::Open)
            || (self.state == MarketState::Resolved)
                != (self.outcome.is_some() && self.resolved_at.is_some())
            || (self.state != MarketState::Resolved
                && (self.outcome.is_some() || self.resolved_at.is_some()))
        {
            return Err("invalid prediction market terms");
        }
        Ok(())
    }
}
/// Anonymous grouped liquidity; no counterparty account, order or hold identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DepthLevel {
    pub side: PredictionSide,
    pub limit_cost: ContractBid,
    pub quantity: DepthQuantity,
}
/// Public detail extends the same summary used by discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketDetail {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub terms: MarketSummary,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub book: Vec<DepthLevel>,
}
/// Caller-bound detail envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedMarket {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub market: MarketDetail,
}
fn side_rank(side: PredictionSide) -> u8 {
    match side {
        PredictionSide::No => 0,
        PredictionSide::Yes => 1,
    }
}
impl OwnedMarket {
    /// Validate the requested market and bounded, distinct, sorted anonymous book.
    ///
    /// # Errors
    /// Rejects mismatched identity, malformed summary, duplicate levels or unavailable liquidity.
    pub fn validate(&self, market: Uuid) -> Result<(), &'static str> {
        self.market.terms.validate()?;
        if self.account_id.is_nil()
            || self.market.terms.id != market
            || self.market.book.len() > 198
            || (!self.market.terms.accepting_orders && !self.market.book.is_empty())
            || self.market.book.windows(2).any(|p| {
                side_rank(p[0].side) > side_rank(p[1].side)
                    || (side_rank(p[0].side) == side_rank(p[1].side)
                        && p[0].limit_cost.amount() <= p[1].limit_cost.amount())
            })
        {
            return Err("invalid prediction book");
        }
        Ok(())
    }
}
/// Owner-only order without private hold/matching operation identities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Order {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub market_id: Uuid,
    pub side: PredictionSide,
    pub limit_cost: ContractBid,
    pub quantity: Amount,
    pub filled_quantity: Amount,
    pub actual_filled_cost: Amount,
    pub unfilled_reservation: Amount,
    pub state: OrderState,
    pub accepted_sequence: Amount,
    pub accepted_at: DateTime<Utc>,
}
/// Filled units remain committed through cancellation of their unfilled remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum OrderState {
    Open,
    Filled,
    Canceled,
    Closed,
    Settled,
}
impl TryFrom<String> for OrderState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "open" => Ok(Self::Open),
            "filled" => Ok(Self::Filled),
            "canceled" => Ok(Self::Canceled),
            "closed" => Ok(Self::Closed),
            "settled" => Ok(Self::Settled),
            _ => Err("invalid string enum value"),
        }
    }
}

impl Order {
    fn validate(&self, market: Option<Uuid>) -> Result<(), &'static str> {
        let qty = self.quantity.integer();
        let filled = self.filled_quantity.integer();
        let bid = self.limit_cost.amount().integer();
        if self.id.is_nil()
            || self.market_id.is_nil()
            || market.is_some_and(|id| id != self.market_id)
            || self.quantity.is_zero()
            || self.accepted_sequence.is_zero()
            || filled > qty
            || Amount::from_biguint(qty * bid).is_err()
            || self.actual_filled_cost.integer() < filled
            || self.actual_filled_cost.integer() > &(filled * bid)
            || (self.state == OrderState::Filled && filled != qty)
            || (self.state == OrderState::Open && filled == qty)
        {
            return Err("invalid prediction order");
        }
        let reserve = if self.state == OrderState::Open {
            (qty - filled) * bid
        } else {
            0u8.into()
        };
        if self.unfilled_reservation.integer() != &reserve {
            return Err("invalid unfilled reservation");
        }
        Ok(())
    }
}
/// Immutable matched pair side; positions cannot be resold by a command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Position {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub market_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub order_id: Uuid,
    pub side: PredictionSide,
    pub quantity: Amount,
    pub actual_cost: Amount,
    /// Fixed100-coin payout per contract, not the aggregate position payout.
    pub fixed_payout: Amount,
    pub matched_at: DateTime<Utc>,
}
impl Position {
    fn validate(&self, market: Option<Uuid>) -> Result<(), &'static str> {
        if self.id.is_nil()
            || self.market_id.is_nil()
            || self.order_id.is_nil()
            || market.is_some_and(|id| id != self.market_id)
            || self.quantity.is_zero()
            || self.fixed_payout != Amount::from(100)
            || self.actual_cost < self.quantity
            || self.actual_cost.integer()
                >= &(self.quantity.integer() * self.fixed_payout.integer())
            || self.actual_cost.integer() % self.quantity.integer() != 0u8.into()
        {
            return Err("invalid prediction position");
        }
        Ok(())
    }
}
/// Exact owner/root scope and the established conservative UUID continuation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct PredictionPage<T> {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<T>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
fn page<T>(
    p: &PredictionPage<T>,
    after: Option<Uuid>,
    limit: usize,
    id: impl Fn(&T) -> Uuid,
) -> Result<(), &'static str> {
    if p.account_id.is_nil() {
        return Err("invalid prediction owner");
    }
    validate_page_ids(&p.items, p.next_cursor, after, limit, id)
}
/// Caller-bound discovery summaries.
pub type MarketPage = PredictionPage<MarketSummary>;
/// Caller-owned order history.
pub type OrderPage = PredictionPage<Order>;
/// Caller-owned immutable filled positions.
pub type PositionPage = PredictionPage<Position>;
impl MarketPage {
    /// Validate market discovery scope, terms and exact paging.
    /// # Errors
    /// Rejects invalid owner, terms, bounds or continuation.
    pub fn validate(&self, after: Option<Uuid>, limit: usize) -> Result<(), &'static str> {
        page(self, after, limit, |v| v.id)?;
        for v in &self.items {
            v.validate()?;
        }
        Ok(())
    }
}
impl OrderPage {
    /// Validate owner-only orders and the optional market filter.
    /// # Errors
    /// Rejects malformed backing/terms, foreign market or continuation.
    pub fn validate(
        &self,
        after: Option<Uuid>,
        limit: usize,
        market: Option<Uuid>,
    ) -> Result<(), &'static str> {
        page(self, after, limit, |v| v.id)?;
        for v in &self.items {
            v.validate(market)?;
        }
        Ok(())
    }
}
impl PositionPage {
    /// Validate owner-only immutable positions and optional market filter.
    /// # Errors
    /// Rejects inconsistent fixed payouts, foreign market or continuation.
    pub fn validate(
        &self,
        after: Option<Uuid>,
        limit: usize,
        market: Option<Uuid>,
    ) -> Result<(), &'static str> {
        page(self, after, limit, |v| v.id)?;
        for v in &self.items {
            v.validate(market)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    #[test]
    fn prediction_projection_fixture_preserves_exact_backing_and_anonymous_large_depth() {
        let f: Value =
            serde_json::from_str(include_str!("../tests/fixtures/prediction-reads.json")).unwrap();
        let market: OwnedMarket = serde_json::from_value(f["market"].clone()).unwrap();
        let id = market.market.terms.id;
        market.validate(id).unwrap();
        assert_eq!(
            serde_json::to_value(&market).unwrap()["market"]["book"],
            f["market"]["market"]["book"]
        );
        serde_json::from_value::<MarketPage>(f["markets"].clone())
            .unwrap()
            .validate(None, 50)
            .unwrap();
        serde_json::from_value::<OrderPage>(f["orders"].clone())
            .unwrap()
            .validate(None, 50, Some(id))
            .unwrap();
        serde_json::from_value::<PositionPage>(f["positions"].clone())
            .unwrap()
            .validate(None, 50, Some(id))
            .unwrap();
        let mut private = f["market"].clone();
        private["market"]["holdId"] = json!(id);
        assert!(serde_json::from_value::<OwnedMarket>(private).is_err());
        let mut wrong = f["orders"].clone();
        wrong["items"][0]["unfilledReservation"] = json!("119");
        assert!(
            serde_json::from_value::<OrderPage>(wrong)
                .unwrap()
                .validate(None, 50, Some(id))
                .is_err()
        );
        let mut wrong = f["positions"].clone();
        wrong["items"][0]["fixedPayout"] = json!("101");
        assert!(
            serde_json::from_value::<PositionPage>(wrong)
                .unwrap()
                .validate(None, 50, Some(id))
                .is_err()
        );
        for bad in ["0".to_owned(), "01".to_owned(), "9".repeat(84)] {
            assert!(DepthQuantity::try_from(bad).is_err());
        }
        assert!(DepthQuantity::try_from("9".repeat(83)).is_ok());
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<MarketState>(&["open", "closed", "resolved"]);
        crate::test_http::string_enum_roundtrip::<Outcome>(&["yes", "no", "void"]);
        crate::test_http::string_enum_roundtrip::<OrderState>(&[
            "open", "filled", "canceled", "closed", "settled",
        ]);
    }
}
