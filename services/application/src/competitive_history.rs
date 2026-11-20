//! Stored owner credit and daily reward facts; reads never score or issue coins.
use crate::{amount::Amount, progression::validate_page};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use uuid::Uuid;
/// Canonical signed 78-digit rating change, distinct from a nonnegative inventory amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatingDelta {
    negative: bool,
    magnitude: Amount,
}
impl<'de> Deserialize<'de> for RatingDelta {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        let text = String::deserialize(input)?;
        let (negative, digits) = text
            .strip_prefix('-')
            .map_or((false, text.as_str()), |s| (true, s));
        let magnitude: Amount = digits.parse().map_err(de::Error::custom)?;
        if negative && magnitude.is_zero() {
            return Err(de::Error::custom("negative zero rating change"));
        }
        Ok(Self {
            negative,
            magnitude,
        })
    }
}
impl Serialize for RatingDelta {
    fn serialize<S: Serializer>(&self, output: S) -> Result<S::Ok, S::Error> {
        output.serialize_str(&format!(
            "{}{}",
            if self.negative { "-" } else { "" },
            self.magnitude
        ))
    }
}
impl RatingDelta {
    fn apply(&self, before: &Amount) -> Result<Amount, crate::amount::AmountError> {
        if self.negative {
            // The persisted124 source floors rating at zero, even for a larger negative delta.
            if &self.magnitude > before {
                Ok(0.into())
            } else {
                before.checked_sub(&self.magnitude)
            }
        } else {
            before.checked_add(&self.magnitude)
        }
    }
}
/// Legacy unranked credits retain an all-null rating tuple, not a fabricated zero rating.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompetitiveCredit {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_result_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub season_id: Option<Uuid>,
    pub win: bool,
    pub completed_at: DateTime<Utc>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub rating_before: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub rating_delta: Option<RatingDelta>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub rating_after: Option<Amount>,
}
/// Exact owner and filter-bound credit page; no public leaderboard is implied.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreditPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<CompetitiveCredit>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl CreditPage {
    /// # Errors
    /// Rejects foreign/filter-inconsistent rows, malformed rating tuples or invalid pagination.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
        format: Option<Uuid>,
        season: Option<Uuid>,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner {
            return Err("invalid credit owner");
        }
        validate_page(
            &self.items.iter().map(|r| r.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for item in &self.items {
            if item.match_result_id.is_nil()
                || item.format_id.is_nil()
                || format.is_some_and(|id| id != item.format_id)
                || season.is_some_and(|id| Some(id) != item.season_id)
            {
                return Err("invalid credit context");
            }
            match (
                &item.season_id,
                &item.rating_before,
                &item.rating_delta,
                &item.rating_after,
            ) {
                (None, None, None, None) => {}
                (Some(_), Some(before), Some(delta), Some(after))
                    if delta.apply(before).ok().as_ref() == Some(after) => {}
                _ => return Err("invalid exact rating transition"),
            }
        }
        Ok(())
    }
}
/// Actual shared UTC reward bucket; no missing row is invented by a read.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DailyWinRecord {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub utc_day: String,
    pub wins: Amount,
    pub issued_coins: Amount,
    pub revision: Amount,
}
/// Bounded owner history of existing daily reward buckets.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DailyWinPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<DailyWinRecord>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl DailyWinPage {
    /// # Errors
    /// Rejects foreign ownership, noncanonical UTC days or inconsistent keyset metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || self
                .items
                .iter()
                .any(|r| !crate::progression::valid_day(&r.utc_day))
        {
            return Err("invalid daily win history");
        }
        validate_page(
            &self.items.iter().map(|r| r.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rating_delta_preserves_signed_exact_values_and_the_stored_zero_floor() {
        for (text, before, expected) in [
            ("-25", 100, 75),
            ("25", 100, 125),
            ("-25", 10, 0),
            ("0", 10, 10),
        ] {
            let delta: RatingDelta = serde_json::from_value(json!(text)).unwrap();
            assert_eq!(serde_json::to_value(&delta).unwrap(), json!(text));
            assert_eq!(delta.apply(&before.into()).unwrap(), expected.into());
        }
        for bad in [
            json!("-0"),
            json!("+1"),
            json!("01"),
            json!("-01"),
            json!("1.0"),
            json!(1),
            json!("-"),
            json!("9".repeat(79)),
        ] {
            assert!(serde_json::from_value::<RatingDelta>(bad).is_err());
        }
        let large = format!("-{}", "9".repeat(78));
        let delta: RatingDelta = serde_json::from_value(json!(large)).unwrap();
        assert_eq!(delta.apply(&0.into()).unwrap(), 0.into());
    }
}
