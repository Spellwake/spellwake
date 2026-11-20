//! Closed trusted supply inputs and historical publisher results. These contracts
//! neither acquire physical evidence nor install a publisher or a public endpoint.
use crate::{
    amount::Amount,
    price_source::EvidenceHash,
    source_protocol::{
        SourceResponseError, accepted_response, checked, original_source_terms,
        parse_utc_microsecond, valid_digest,
    },
};
use serde::{Deserialize, Deserializer, de};
use serde_json::value::RawValue;
use std::collections::BTreeSet;
use uuid::Uuid;
mod manifests;
mod opening;
pub use manifests::{BaselineManifest, PhysicalIntroduction, SupplyBallot};
pub use opening::{BoosterOpening, QualifyBoosterOpening, QualifyBoosterRecipe};
#[cfg(test)]
mod tests;

fn bounded(value: &Amount, digits: usize, positive: bool) -> bool {
    value.to_string().len() <= digits && (!positive || !value.is_zero())
}
fn unique<T: Ord>(values: impl Iterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    values.into_iter().all(|value| seen.insert(value))
}
fn encoded_objects<'de, T: de::DeserializeOwned, D: Deserializer<'de>>(
    input: D,
) -> Result<Vec<T>, D::Error> {
    let text = String::deserialize(input)?;
    serde_json::from_str::<Vec<crate::provider_json::Object<T>>>(&text)
        .map(|xs| xs.into_iter().map(|x| x.0).collect())
        .map_err(de::Error::custom)
}
/// One immutable baseline product publication, ordered by product identity.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaselineWindow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub recipe_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub product_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub window_id: Uuid,
    pub quantity: Amount,
}
/// Baseline publisher's actual string-encoded window collection.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaselineAck {
    pub quantity_per_product: Amount,
    #[serde(deserialize_with = "encoded_objects")]
    pub windows: Vec<BaselineWindow>,
}
/// Exact physical-source result disposition, never a unit/measurement inference.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub enum PhysicalDisposition {
    Unavailable,
    Zero,
    Published,
}
impl TryFrom<String> for PhysicalDisposition {
    type Error = SourceResponseError;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        match text.as_str() {
            "unavailable" => Ok(Self::Unavailable),
            "zero" => Ok(Self::Zero),
            "published" => Ok(Self::Published),
            _ => Err(SourceResponseError::context()),
        }
    }
}
/// Coverage is the database's bare digest. Empty window is a required sentinel.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PhysicalAck {
    pub coverage: String,
    pub quantity: Amount,
    #[serde(deserialize_with = "crate::identifiers::empty_uuid")]
    pub window_id: Option<Uuid>,
    pub disposition: PhysicalDisposition,
}
/// Frozen ballot receipt, distinct from a subsequently computed allocation.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BallotAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub candidates: Amount,
    pub cutoff: String,
}
/// One quarterly window; SQL orders by source set/product, not recipe UUID.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarterlyWindow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub recipe_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub window_id: Uuid,
    pub quantity: Amount,
}
/// Historical quarterly allocation, with conserved whole quantities.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuarterlyAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub allocation_ref: EvidenceHash,
    pub quantity: Amount,
    #[serde(deserialize_with = "encoded_objects")]
    pub windows: Vec<QuarterlyWindow>,
}
///195 immutable season target; no player-selected population or winner list.
pub struct FinalizeQuarterlySupply {
    pub season_id: Uuid,
}
impl FinalizeQuarterlySupply {
    /// Decode the stored allocation for this season, preserving original order.
    /// # Errors
    /// Rejects foreign identities, duplicates, nonpositive windows or lost quantity.
    pub fn decode_response(&self, bytes: &[u8]) -> Result<QuarterlyAck, SourceResponseError> {
        let ack: QuarterlyAck = accepted_response(bytes)?;
        let sum = ack
            .windows
            .iter()
            .try_fold(Amount::from(0), |sum, x| sum.checked_add(&x.quantity));
        let valid = !self.season_id.is_nil()
            && ack.resource_id == self.season_id
            && ack.windows.len() <= 5000
            && unique(ack.windows.iter().map(|x| x.recipe_id))
            && unique(ack.windows.iter().map(|x| x.window_id))
            && ack.windows.iter().all(|x| !x.quantity.is_zero())
            && sum.is_ok_and(|sum| sum == ack.quantity);
        checked(valid, ack)
    }
}
