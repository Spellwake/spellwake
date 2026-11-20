//! Bounded trusted lifecycle delivery; clients cannot supply a clock or outcome.

use super::{CollectorError, PriceCollector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MaintenancePage {
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    items: Vec<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    next_cursor: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum MarketDisposition {
    NotDue,
    Pending,
    Final {
        outcome: Outcome,
        #[serde(rename = "operationId", deserialize_with = "crate::identifiers::uuid")]
        operation_id: Uuid,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
enum Outcome {
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

/// Counts acknowledge calls in this attempt, not historical settlement totals.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaintenanceBatch {
    pub processed: u32,
    pub settled: u32,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
    pub more: bool,
}

impl PriceCollector {
    /// At most100 independent market transactions. A failed/lost acknowledgement
    /// stops this attempt; retry discovers durable remaining work again.
    pub fn maintain_predictions(
        &self,
        after: Option<Uuid>,
    ) -> Result<MaintenanceBatch, CollectorError> {
        if after.is_some_and(|id| id.is_nil()) {
            return Err(CollectorError::InvalidCursor);
        }
        let page: crate::provider_json::Object<MaintenancePage> = self.rpc(
            "read_prediction_maintenance_page",
            &json!({"p_after":after,"p_limit":100}),
        )?;
        let page = page.0;
        let mut previous = after;
        if page.items.len() > 100 {
            return Err(CollectorError::Backend);
        }
        for id in &page.items {
            if id.is_nil() || previous.is_some_and(|previous| *id <= previous) {
                return Err(CollectorError::Backend);
            }
            previous = Some(*id);
        }
        let more = page.items.len() == 100;
        if page.next_cursor
            != if more {
                page.items.last().copied()
            } else {
                None
            }
        {
            return Err(CollectorError::Backend);
        }
        let mut result = MaintenanceBatch {
            processed: 0,
            settled: 0,
            next_cursor: page.next_cursor,
            more,
        };
        for id in page.items {
            let disposition: MarketDisposition =
                self.rpc("maintain_prediction_market", &json!({"p_market":id}))?;
            match disposition {
                MarketDisposition::Final {
                    outcome,
                    operation_id,
                } => {
                    if operation_id.is_nil() {
                        return Err(CollectorError::Backend);
                    }
                    let _ = outcome; // SQL's typed terminal outcome is validated, never recomputed here.
                    result.settled += 1;
                }
                MarketDisposition::NotDue | MarketDisposition::Pending => {}
            }
            result.processed += 1;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_variants::<Outcome>(&[
            ("yes", "Yes"),
            ("no", "No"),
            ("void", "Void"),
        ]);
    }
}

#[cfg(test)]
mod market_disposition_wire_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn tagged_objects_remain_valid_with_strict_scalar_outcomes() {
        for state in ["not_due", "pending"] {
            assert!(serde_json::from_value::<MarketDisposition>(json!({"state":state})).is_ok());
            for malformed in [json!({"state":{state:null}}), json!({"state":[state]})] {
                assert!(serde_json::from_value::<MarketDisposition>(malformed).is_err());
            }
        }
        for outcome in ["yes", "no", "void"] {
            let valid = json!({"state":"final","outcome":outcome,"operationId":Uuid::new_v4()});
            assert!(serde_json::from_value::<MarketDisposition>(valid.clone()).is_ok());
            let mut malformed = valid;
            malformed["outcome"] = json!({outcome:null});
            assert!(serde_json::from_value::<MarketDisposition>(malformed).is_err());
        }
    }
}
