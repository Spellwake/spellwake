//! Caller-owned exchange history with bounded pages and exact settlement summaries.
use crate::{
    account::PublicAccountIdentity,
    amount::Amount,
    inventory::AssetIdentity,
    policy::{Rational, outgoing_allowed},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Keep worst-case bundles and exact valuation components within the RPC byte limit.
pub const EXCHANGE_PAGE_LIMIT: usize = 8;
/// Frozen total and the exact valuation policy/catalogue epoch used at settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValuationTotal {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub policy_id: Uuid,
    pub catalogue_revision: Amount,
    pub total: Rational,
}
/// Accepted two-sided assessment without private per-line recipe/source evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettlementSummary {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub outgoing: ValuationTotal,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub incoming: ValuationTotal,
    pub outgoing_allowed: bool,
    pub incoming_allowed: bool,
    pub sender_restricted: bool,
    pub recipient_restricted: bool,
}
impl SettlementSummary {
    fn validate(&self) -> Result<(), &'static str> {
        if self.outgoing.policy_id.is_nil()
            || self.outgoing.policy_id != self.incoming.policy_id
            || self.outgoing.catalogue_revision != self.incoming.catalogue_revision
            || self.outgoing_allowed != outgoing_allowed(&self.outgoing.total, &self.incoming.total)
            || self.incoming_allowed != outgoing_allowed(&self.incoming.total, &self.outgoing.total)
            || (self.sender_restricted && !self.outgoing_allowed)
            || (self.recipient_restricted && !self.incoming_allowed)
        {
            return Err("invalid accepted settlement summary");
        }
        Ok(())
    }
}
/// Offered quantity of an asset definition, independent of inventory and hold rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TradeAsset {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub asset_id: Uuid,
    pub quantity: Amount,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub asset: AssetIdentity,
}
/// Stored offer lifecycle; it does not grant permission for another action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum TradeState {
    Offered,
    Accepted,
    Rejected,
    Canceled,
}
impl TryFrom<String> for TradeState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "offered" => Ok(Self::Offered),
            "accepted" => Ok(Self::Accepted),
            "rejected" => Ok(Self::Rejected),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Sealed immutable terms and historical settlement, visible only to participants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TradeRecord {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub sender: PublicAccountIdentity,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub recipient: PublicAccountIdentity,
    pub revision: Amount,
    pub state: TradeState,
    pub created_at: DateTime<Utc>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub give: Vec<TradeAsset>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub request: Vec<TradeAsset>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub settlement: Option<SettlementSummary>,
}
impl TradeRecord {
    fn validate(&self, actor: Uuid) -> Result<(), &'static str> {
        self.sender.validate()?;
        self.recipient.validate()?;
        if actor.is_nil()
            || self.id.is_nil()
            || self.sender.id == self.recipient.id
            || ![self.sender.id, self.recipient.id].contains(&actor)
            || (self.sender.id == actor && self.sender.is_deleted)
            || (self.recipient.id == actor && self.recipient.is_deleted)
            || self.revision != Amount::from(1)
            || self.give.len() > 100
            || self.request.len() > 100
            || (self.give.is_empty() && self.request.is_empty())
            || (self.state == TradeState::Accepted) != self.settlement.is_some()
        {
            return Err("invalid owned trade record");
        }
        for bundle in [&self.give, &self.request] {
            if bundle
                .windows(2)
                .any(|pair| pair[0].asset_id >= pair[1].asset_id)
            {
                return Err("invalid trade bundle ordering");
            }
            for line in bundle {
                if line.asset_id.is_nil() || line.quantity.is_zero() {
                    return Err("invalid trade asset line");
                }
                line.asset.validate()?;
            }
        }
        if let Some(settlement) = &self.settlement {
            settlement.validate()?;
            let zero = Rational::whole(Amount::from(0));
            if (settlement.outgoing.total == zero) != self.give.is_empty()
                || (settlement.incoming.total == zero) != self.request.is_empty()
            {
                return Err("invalid trade bundle valuation");
            }
        }
        Ok(())
    }
}
/// Whole-lot listing lifecycle; changed terms require canceling and relisting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ListingState {
    Listed,
    Sold,
    Canceled,
}
impl TryFrom<String> for ListingState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "listed" => Ok(Self::Listed),
            "sold" => Ok(Self::Sold),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Private sale facts returned only to the seller or actual buyer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListingParticipant {
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub buyer: Option<PublicAccountIdentity>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub paid_coins: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub settlement: Option<SettlementSummary>,
}
/// Listing terms with an explicit optional participant-only fact group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListingRecord {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub seller: PublicAccountIdentity,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub asset_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub asset: AssetIdentity,
    pub quantity: Amount,
    pub total_price: Amount,
    pub state: ListingState,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub participant: Option<ListingParticipant>,
}
impl ListingRecord {
    fn validate(&self, actor: Uuid) -> Result<(), &'static str> {
        self.seller.validate()?;
        self.asset.validate()?;
        if actor.is_nil()
            || self.id.is_nil()
            || self.asset_id.is_nil()
            || self.quantity.is_zero()
            || self.total_price.is_zero()
            || self.revision.is_zero()
            || matches!(self.asset, AssetIdentity::Coin { .. })
            || (self.seller.id == actor && self.seller.is_deleted)
        {
            return Err("invalid market listing");
        }
        match &self.participant {
            None if self.state != ListingState::Listed || self.seller.id == actor => {
                return Err("invalid public listing visibility");
            }
            None => {}
            Some(participant) => {
                if let Some(buyer) = &participant.buyer {
                    buyer.validate()?;
                    if buyer.id == self.seller.id || (buyer.id == actor && buyer.is_deleted) {
                        return Err("invalid listing buyer");
                    }
                }
                if self.seller.id != actor
                    && participant
                        .buyer
                        .as_ref()
                        .is_none_or(|buyer| buyer.id != actor)
                {
                    return Err("foreign listing participant");
                }
                if self.state == ListingState::Sold {
                    if participant.buyer.is_none()
                        || participant.paid_coins.as_ref() != Some(&self.total_price)
                        || participant.completed_at.is_none()
                    {
                        return Err("incomplete listing settlement");
                    }
                    let settlement = participant
                        .settlement
                        .as_ref()
                        .ok_or("missing listing valuation")?;
                    settlement.validate()?;
                    if settlement.incoming.total != Rational::whole(self.total_price.clone())
                        || settlement.outgoing.total == Rational::whole(Amount::from(0))
                    {
                        return Err("invalid listing valuation");
                    }
                } else if participant.buyer.is_some()
                    || participant.paid_coins.is_some()
                    || participant.completed_at.is_some()
                    || participant.settlement.is_some()
                {
                    return Err("unsettled listing has terminal facts");
                }
            }
        }
        Ok(())
    }
}

/// Canonical caller envelope for an exact participant-owned trade.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedTrade {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub trade: TradeRecord,
}
impl OwnedTrade {
    /// Validate participant scope and the exact requested trade.
    ///
    /// # Errors
    /// Rejects foreign identities or inconsistent terms/settlement groups.
    pub fn validate(&self, requested: Uuid) -> Result<(), &'static str> {
        if requested.is_nil() || self.trade.id != requested {
            return Err("foreign trade result");
        }
        self.trade.validate(self.account_id)
    }
}
/// Canonical caller envelope for a visible public or participated listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnedListing {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub listing: ListingRecord,
}
impl OwnedListing {
    /// Validate public/participant visibility and the requested listing.
    ///
    /// # Errors
    /// Rejects foreign identities, private facts in public records or inconsistent settlements.
    pub fn validate(&self, requested: Uuid) -> Result<(), &'static str> {
        if requested.is_nil() || self.listing.id != requested {
            return Err("foreign listing result");
        }
        self.listing.validate(self.account_id)
    }
}
/// Caller-bound UUID-ordered records; pagination does not freeze later requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct ExchangePage<T> {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<T>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl<T> ExchangePage<T> {
    fn validate_cursor(
        &self,
        after: Option<Uuid>,
        limit: usize,
        id: impl Fn(&T) -> Uuid,
    ) -> Result<(), &'static str> {
        if self.account_id.is_nil()
            || !(1..=EXCHANGE_PAGE_LIMIT).contains(&limit)
            || after.is_some_and(|id| id.is_nil())
        {
            return Err("invalid exchange page scope");
        }
        crate::store::validate_page_ids(&self.items, self.next_cursor, after, limit, id)
    }
}
/// Caller-participated trade history, optionally filtered by lifecycle state.
pub type TradePage = ExchangePage<TradeRecord>;
/// Public listed lots or the caller's participated market history.
pub type ListingPage = ExchangePage<ListingRecord>;
impl TradePage {
    /// Validate caller ownership, state filter and complete stable cursor ordering.
    ///
    /// # Errors
    /// Rejects oversized, foreign, misordered or filter-inconsistent pages.
    pub fn validate(
        &self,
        after: Option<Uuid>,
        limit: usize,
        state: Option<TradeState>,
    ) -> Result<(), &'static str> {
        self.validate_cursor(after, limit, |item| item.id)?;
        for item in &self.items {
            item.validate(self.account_id)?;
            if state.is_some_and(|state| state != item.state) {
                return Err("trade state filter mismatch");
            }
        }
        Ok(())
    }
}
impl ListingPage {
    /// Validate caller history/public scope and stable cursor ordering.
    ///
    /// # Errors
    /// Rejects foreign history, terminal public records or inconsistent cursors/settlements.
    pub fn validate(
        &self,
        after: Option<Uuid>,
        limit: usize,
        owned: bool,
    ) -> Result<(), &'static str> {
        self.validate_cursor(after, limit, |item| item.id)?;
        for item in &self.items {
            item.validate(self.account_id)?;
            if (owned && item.participant.is_none())
                || (!owned && item.state != ListingState::Listed)
            {
                return Err("market page scope mismatch");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn fixtures() -> Value {
        serde_json::from_str(include_str!("../tests/fixtures/exchange-reads.json")).unwrap()
    }
    #[test]
    fn exchange_records_reject_foreign_identity_private_fields_and_inconsistent_terminal_groups() {
        let f = fixtures();
        for name in ["offeredTrade", "acceptedTrade"] {
            let value = serde_json::from_value::<OwnedTrade>(f[name].clone()).unwrap();
            value.validate(value.trade.id).unwrap();
        }
        for name in ["listedListing", "soldListing"] {
            let value = serde_json::from_value::<OwnedListing>(f[name].clone()).unwrap();
            value.validate(value.listing.id).unwrap();
        }
        let mut missing = f["listedListing"].clone();
        missing["listing"]
            .as_object_mut()
            .unwrap()
            .remove("participant");
        assert!(serde_json::from_value::<OwnedListing>(missing).is_err());
        let mut private = f["offeredTrade"].clone();
        private["trade"]["senderHoldId"] = json!(Uuid::new_v4());
        assert!(serde_json::from_value::<OwnedTrade>(private).is_err());
        let mut missing_settlement = f["acceptedTrade"].clone();
        missing_settlement["trade"]["settlement"] = Value::Null;
        let mut foreign_trade = f["offeredTrade"].clone();
        foreign_trade["accountId"] = json!(Uuid::new_v4());
        let mut duplicate = f["offeredTrade"].clone();
        let line = duplicate["trade"]["give"][0].clone();
        duplicate["trade"]["give"]
            .as_array_mut()
            .unwrap()
            .push(line);
        for value in [missing_settlement, foreign_trade, duplicate] {
            let parsed = serde_json::from_value::<OwnedTrade>(value).unwrap();
            assert!(parsed.validate(parsed.trade.id).is_err());
        }
        let mut foreign = f["soldListing"].clone();
        foreign["accountId"] = json!(Uuid::new_v4());
        let parsed = serde_json::from_value::<OwnedListing>(foreign).unwrap();
        assert!(parsed.validate(parsed.listing.id).is_err());
        let mut bad = f["acceptedTrade"].clone();
        bad["trade"]["settlement"]["outgoingAllowed"] = json!(false);
        let parsed = serde_json::from_value::<OwnedTrade>(bad).unwrap();
        assert!(parsed.validate(parsed.trade.id).is_err());
        let mut name = f["offeredTrade"].clone();
        name["trade"]["recipient"]["isDeleted"] = json!(true);
        let parsed = serde_json::from_value::<OwnedTrade>(name).unwrap();
        assert!(parsed.validate(parsed.trade.id).is_err());
        let mut fraction = f["acceptedTrade"].clone();
        fraction["trade"]["settlement"]["outgoing"]["total"] =
            json!({"numerator":"200","denominator":"2"});
        assert!(serde_json::from_value::<OwnedTrade>(fraction).is_err());
    }
    #[test]
    fn maximal_exchange_wire_page_stays_below_the_existing_rpc_byte_limit() {
        let f = fixtures();
        let mut trade = f["acceptedTrade"]["trade"].clone();
        let mut ids: Vec<_> = (0..100).map(|_| Uuid::new_v4()).collect();
        ids.sort();
        let lines:Vec<_>=ids.iter().map(|id|json!({"assetId":id,"quantity":"9".repeat(78),"asset":{"kind":"card","printingId":id}})).collect();
        trade["give"] = json!(lines);
        trade["request"] = json!(lines);
        let fraction = json!({"numerator":format!("1{}1","0".repeat(crate::policy::MAX_RATIONAL_DIGITS-2)),"denominator":format!("1{}","0".repeat(crate::policy::MAX_RATIONAL_DIGITS-1))});
        trade["settlement"]["outgoing"]["total"] = fraction.clone();
        trade["settlement"]["incoming"]["total"] = fraction;
        let mut ids: Vec<_> = (0..EXCHANGE_PAGE_LIMIT).map(|_| Uuid::new_v4()).collect();
        ids.sort();
        let items: Vec<_> = ids
            .iter()
            .map(|id| {
                let mut value = trade.clone();
                value["id"] = json!(id);
                value
            })
            .collect();
        let data = json!({"accountId":f["acceptedTrade"]["accountId"],"items":items,"nextCursor":ids.last()});
        let page: TradePage = serde_json::from_value(data.clone()).unwrap();
        page.validate(None, EXCHANGE_PAGE_LIMIT, None).unwrap();
        let response = serde_json::to_vec(&json!({"status":"success","data":data})).unwrap();
        assert!(
            response.len() < 1024 * 1024,
            "wire envelope was {} bytes",
            response.len()
        );
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<TradeState>(&[
            "offered", "accepted", "rejected", "canceled",
        ]);
        crate::test_http::string_enum_roundtrip::<ListingState>(&["listed", "sold", "canceled"]);
    }
}
