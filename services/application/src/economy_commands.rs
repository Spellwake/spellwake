//! Authenticated economy intent reuses canonical settlement inputs and receipts.
use crate::{
    commands::CommandAck,
    exchange::{BuyListing, CancelListing, CreateListing, CreateTrade, RespondTrade},
    packs::OpenPack,
    supply::BuyPacks,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// No actor, valuation, hold identifier, seed or outcome is accepted from clients.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    content = "command",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum EconomyCommand {
    TradeCreate(CreateTrade),
    TradeResponse(RespondTrade),
    ListingCreate(CreateListing),
    ListingCancel(CancelListing),
    ListingBuy(BuyListing),
    PackBuy(BuyPacks),
    PackOpen(OpenPack),
}
impl<'de> Deserialize<'de> for EconomyCommand {
    fn deserialize<D: serde::Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let wire: crate::provider_json::CommandObject = crate::provider_json::object(input)?;
        match wire.kind.as_str() {
            "trade_create" => wire.payload().map(Self::TradeCreate),
            "trade_response" => wire.payload().map(Self::TradeResponse),
            "listing_create" => wire.payload().map(Self::ListingCreate),
            "listing_cancel" => wire.payload().map(Self::ListingCancel),
            "listing_buy" => wire.payload().map(Self::ListingBuy),
            "pack_buy" => wire.payload().map(Self::PackBuy),
            "pack_open" => wire.payload().map(Self::PackOpen),
            _ => Err(serde::de::Error::custom("invalid command kind")),
        }
    }
}

impl EconomyCommand {
    /// Validate exact existing intent bounds and all public identities.
    ///
    /// # Errors
    /// Rejects nil identities, empty offers, duplicate/zero bundle lines or invalid accepted terms.
    pub fn validate(&self) -> Result<(), &'static str> {
        let ids = match self {
            Self::TradeCreate(command) => {
                command.validate()?;
                if command
                    .give
                    .as_slice()
                    .iter()
                    .chain(command.request.as_slice())
                    .any(|line| line.asset_id.is_nil())
                {
                    return Err("economy asset identity required");
                }
                [
                    command.request_id,
                    command.recipient_id,
                    command.recipient_id,
                ]
            }
            Self::TradeResponse(command) => {
                [command.request_id, command.trade_id, command.trade_id]
            }
            Self::ListingCreate(command) => {
                command.validate()?;
                [
                    command.request_id,
                    command.lot.asset_id,
                    command.lot.asset_id,
                ]
            }
            Self::ListingCancel(command) => {
                [command.request_id, command.listing_id, command.listing_id]
            }
            Self::ListingBuy(command) => {
                command.validate()?;
                [command.request_id, command.listing_id, command.listing_id]
            }
            Self::PackBuy(command) => {
                command.validate()?;
                [
                    command.request_id,
                    command.recipe_id,
                    command.expected_policy_id,
                ]
            }
            Self::PackOpen(command) => {
                command.validate()?;
                [
                    command.request_id,
                    command.inventory_asset_id,
                    command.inventory_asset_id,
                ]
            }
        };
        if ids.iter().any(uuid::Uuid::is_nil) {
            return Err("economy request and target identities required");
        }
        Ok(())
    }

    pub(crate) fn valid_ack(&self, ack: &CommandAck) -> bool {
        !ack.resource_id.is_nil()
            && match self {
                Self::TradeResponse(command) => ack.resource_id == command.trade_id,
                Self::ListingCancel(command) => ack.resource_id == command.listing_id,
                Self::ListingBuy(command) => ack.resource_id == command.listing_id,
                _ => true,
            }
    }

    pub(crate) fn failure(&self, errors: &BTreeMap<String, String>) -> bool {
        if errors.len() != 1 {
            return false;
        }
        let Some((field, reason)) = errors.iter().next().map(|(k, v)| (k.as_str(), v.as_str()))
        else {
            return false;
        };
        if (field, reason) == ("account", "account unavailable") {
            return true;
        }
        match self {
            Self::TradeCreate(_) => matches!(
                (field, reason),
                ("contact", "trade contact unavailable")
                    | (
                        "assets",
                        "invalid trade bundle"
                            | "trade assets unavailable"
                            | "not enough available offered assets"
                    )
            ),
            Self::TradeResponse(_) => matches!(
                (field, reason),
                (
                    "trade",
                    "trade action unavailable" | "trade is already terminal"
                ) | ("contact", "trade contact unavailable")
                    | ("assets", "trade assets no longer available")
                    | (
                        "value",
                        "restricted outgoing value exceeds the allowed limit"
                    )
            ),
            Self::ListingCreate(_) => matches!(
                (field, reason),
                ("listing", "invalid whole-lot listing")
                    | ("assets", "not enough available assets")
            ),
            Self::ListingCancel(_) => (field, reason) == ("listing", "listing unavailable"),
            Self::ListingBuy(_) => matches!(
                (field, reason),
                (
                    "listing",
                    "listing unavailable" | "listing terms or state changed"
                ) | ("account", "market accounts unavailable")
                    | ("assets", "market assets no longer available")
                    | (
                        "value",
                        "restricted outgoing value exceeds the allowed limit"
                    )
            ),
            Self::PackBuy(_) => matches!(
                (field, reason),
                (
                    "product",
                    "opening recipe not qualified" | "product unavailable"
                ) | ("quote", "invalid purchase terms")
                    | ("quote", "stock or price changed; request a new quote")
                    | ("coins", "not enough available coins")
            ),
            Self::PackOpen(_) => matches!(
                (field, reason),
                (
                    "pack",
                    "owned pack unavailable"
                        | "opening recipe not qualified"
                        | "no unheld pack available"
                )
            ),
        }
    }
}
