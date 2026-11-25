//! Spellwake application contracts and policy, separate from native game rules.
//! Persistence and host integration must preserve source-bound atomic operations.

pub mod account;
pub mod actor;
pub mod admission_ports;
pub mod ai_ante_port;
pub mod ai_runtime;
pub mod amount;
pub mod ante;
pub mod api;
mod backend;
mod capability;
pub mod catalogue;
pub mod catalogue_publisher;
pub mod catalogue_source;
pub mod commands;
pub mod competitive_history;
pub mod contact;
pub mod controller_ante;
pub mod controller_ports;
pub mod controller_possession;
pub mod cosmetics;
pub mod custom_cards;
pub mod deadline_ports;
pub mod decks;
pub mod deletion;
pub mod deletion_delivery;
pub mod deletion_workflow;
pub mod economy_commands;
pub mod exchange;
pub mod exchange_reads;
pub mod formats;
pub mod game_chat;
pub mod handoffs;
mod http_transport;
mod identifiers;
pub mod inventory;
pub mod match_history;
pub mod matchmaking;
#[cfg(test)]
#[path = "../build_support/native_identity.rs"]
mod native_build_identity_tests;
pub mod native_game;
pub mod notifications;
mod opening_choice;
pub mod pack_shop;
pub mod packs;
pub mod patreon;
pub mod patreon_source_ports;
mod player_actor;
pub mod player_wire;
pub mod policy;
pub mod prediction_publication;
pub mod prediction_reads;
pub mod predictions;
pub mod price_collector;
pub mod price_source;
pub mod price_workflow;
mod private_storage;
pub mod profile_art;
pub mod progression;
mod provider_json;
pub mod qualified_intents;
pub mod recovery_ports;
pub mod retirement_resources;
pub mod room_controls;
pub mod rooms;
pub mod social;
pub mod source_protocol;
pub mod store;
pub mod stripe;
pub mod stripe_workflow;
pub mod supply;
pub mod supply_ballot;
pub mod supply_source_ports;
pub mod terminal_consumers;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Every application response distinguishes business rejection from service failure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApiResponse<T> {
    Success {
        data: T,
    },
    Fail {
        #[serde(deserialize_with = "provider_json::failure_fields")]
        data: BTreeMap<String, String>,
    },
    Error {
        message: String,
        code: String,
    },
}

pub mod temporal_runtime;

#[cfg(test)]
mod test_http;

#[cfg(test)]
mod response_tests;

/// Decode an object-valued wire contract directly from bytes, preserving duplicate fields.
///
/// # Errors
/// Rejects non-object JSON and any shape, duplicate or field error reported by the target DTO.
/// Diagnostic errors may contain untrusted values; public boundaries must map them safely.
pub fn decode_wire_object<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, serde_json::Error> {
    serde_json::from_slice::<provider_json::Object<T>>(bytes).map(|value| value.0)
}
