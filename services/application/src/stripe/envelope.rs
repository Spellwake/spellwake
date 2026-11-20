//! Selected Stripe event fields. Vendor additions are intentionally ignored.
pub(super) use crate::provider_json::{
    Array, Object, boolean, integer, null, optional_integer, optional_string, string,
};
use serde::Deserialize;

// Vendor additions are ignored intentionally. App-owned DTOs remain strict.
#[derive(Deserialize)]
pub(super) struct Envelope {
    #[serde(deserialize_with = "string")]
    pub id: String,
    #[serde(deserialize_with = "string")]
    pub object: String,
    #[serde(rename = "type", deserialize_with = "string")]
    pub kind: String,
    #[serde(deserialize_with = "integer")]
    pub created: i64,
    #[serde(rename = "livemode", deserialize_with = "boolean")]
    pub live: bool,
    #[serde(deserialize_with = "string")]
    pub api_version: String,
    pub data: Object<EventData>,
}
#[derive(Deserialize)]
pub(super) struct EventData {
    pub object: Object<EventObject>,
}
#[derive(Deserialize)]
pub(super) struct EventObject {
    #[serde(default, deserialize_with = "optional_string")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "optional_string")]
    pub object: Option<String>,
}

pub(super) fn parse(body: &[u8]) -> Result<Envelope, serde_json::Error> {
    serde_json::from_slice::<Object<Envelope>>(body).map(|envelope| envelope.0)
}
