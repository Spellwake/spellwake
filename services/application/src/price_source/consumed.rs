//! Duplicate checks for the union of fields consumed by catalogue and price projections.
//! Values keep their existing downstream validation; vendor additions are deliberately ignored.
use super::SourceError;
use crate::provider_json::Object;
use serde::{Deserialize, de::IgnoredAny};
use serde_json::value::RawValue;

#[derive(Deserialize)]
struct Card {
    #[serde(rename = "object")]
    _object: Option<IgnoredAny>,
    #[serde(rename = "id")]
    _id: Option<IgnoredAny>,
    #[serde(rename = "oracle_id")]
    _oracle: Option<IgnoredAny>,
    #[serde(rename = "name")]
    _name: Option<IgnoredAny>,
    #[serde(rename = "set_id")]
    _set_id: Option<IgnoredAny>,
    #[serde(rename = "set")]
    _set: Option<IgnoredAny>,
    #[serde(rename = "set_name")]
    _set_name: Option<IgnoredAny>,
    #[serde(rename = "collector_number")]
    _collector: Option<IgnoredAny>,
    #[serde(rename = "lang")]
    _language: Option<IgnoredAny>,
    #[serde(rename = "rarity")]
    _rarity: Option<IgnoredAny>,
    #[serde(rename = "finishes")]
    _finishes: Option<IgnoredAny>,
    #[serde(rename = "layout")]
    _layout: Option<IgnoredAny>,
    prices: Option<Box<RawValue>>,
    card_faces: Option<Box<RawValue>>,
}
#[derive(Deserialize)]
struct Prices {
    #[serde(rename = "usd")]
    _usd: Option<IgnoredAny>,
    #[serde(rename = "usd_foil")]
    _foil: Option<IgnoredAny>,
    #[serde(rename = "usd_etched")]
    _etched: Option<IgnoredAny>,
}
#[derive(Deserialize)]
struct Face {
    #[serde(rename = "name")]
    _name: Option<IgnoredAny>,
}

/// Check original entries before Value can normalize them. Unsupported value shapes
/// still reach their existing projection owners rather than becoming invented source failures.
pub(super) fn validate(bytes: &[u8]) -> Result<(), SourceError> {
    let Object(card) =
        serde_json::from_slice::<Object<Card>>(bytes).map_err(SourceError::invalid_artifact)?;
    if let Some(prices) = card
        .prices
        .filter(|raw| raw.get().trim_start().starts_with('{'))
    {
        serde_json::from_str::<Object<Prices>>(prices.get())
            .map_err(SourceError::invalid_artifact)?;
    }
    if let Some(faces) = card
        .card_faces
        .filter(|raw| raw.get().trim_start().starts_with('['))
    {
        let faces: Vec<Box<RawValue>> =
            serde_json::from_str(faces.get()).map_err(SourceError::invalid_artifact)?;
        for face in faces
            .iter()
            .filter(|raw| raw.get().trim_start().starts_with('{'))
        {
            serde_json::from_str::<Object<Face>>(face.get())
                .map_err(SourceError::invalid_artifact)?;
        }
    }
    Ok(())
}
