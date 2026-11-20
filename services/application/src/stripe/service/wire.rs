//! Selected private RPC projections. These are not provider payment proofs.
use crate::amount::Amount;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub(super) enum EffectDisposition {
    Applied,
    Duplicate,
    Noop,
    Stale,
}
impl TryFrom<String> for EffectDisposition {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "applied" => Ok(Self::Applied),
            "duplicate" => Ok(Self::Duplicate),
            "noop" => Ok(Self::Noop),
            "stale" => Ok(Self::Stale),
            _ => Err("invalid string enum value"),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct EffectResult {
    pub disposition: EffectDisposition,
    pub purchase_id: Uuid,
    pub revision: Amount,
    pub state: String,
    #[serde(deserialize_with = "Option::deserialize")]
    pub grant_id: Option<Uuid>,
}
impl EffectResult {
    pub fn matches(&self, purchase: Uuid, revision: &str) -> bool {
        if self.disposition == EffectDisposition::Stale {
            return false;
        }
        let Ok(mut expected) = revision.parse::<Amount>() else {
            return false;
        };
        if self.disposition == EffectDisposition::Applied {
            let Ok(next) = expected.checked_add(&Amount::from(1)) else {
                return false;
            };
            expected = next;
        }
        self.purchase_id == purchase
            && !purchase.is_nil()
            && self.revision == expected
            && self.grant_id.is_none_or(|id| !id.is_nil())
            && matches!(
                self.state.as_str(),
                "creating" | "pending" | "paid" | "canceled" | "refunded" | "reversed"
            )
    }
}

// Other stored capture/row fields remain private and intentionally unconsumed.
// Decode selected keys directly so duplicates are not normalized through Value.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Notification {
    pub event_type: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub capture: NotificationCapture,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct NotificationCapture {
    pub target_id: Option<String>,
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<EffectDisposition>(&[
            "applied",
            "duplicate",
            "noop",
            "stale",
        ]);
    }
}
