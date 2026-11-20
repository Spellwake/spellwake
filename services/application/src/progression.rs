//! Owner-scoped stored progression; reads never assign, reward or finalize.
use crate::amount::Amount;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Validate ascending keyset pagination without assuming total counts or page membership of links.
pub(crate) fn validate_page(
    ids: &[Uuid],
    after: Option<Uuid>,
    limit: usize,
    cursor: Option<Uuid>,
) -> Result<(), &'static str> {
    if !(1..=100).contains(&limit)
        || ids.len() > limit
        || ids
            .iter()
            .any(|id| id.is_nil() || after.is_some_and(|previous| *id <= previous))
        || ids.windows(2).any(|pair| pair[0] >= pair[1])
        || cursor
            != if ids.len() == limit {
                ids.last().copied()
            } else {
                None
            }
    {
        return Err("invalid keyset page");
    }
    Ok(())
}
pub(crate) fn valid_day(value: &str) -> bool {
    value.len() == 10 && NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
}
/// Qualified definition projection; reference-only unavailable definitions remain absent.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestObjective {
    pub definition_key: String,
    pub revision: String,
    pub objective_ref: String,
    pub target: Amount,
    pub reward_coins: Amount,
}
/// Immutable assignment identity and current counted state, without reward/source authority.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestAssignment {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub definition_id: Uuid,
    pub source_day: String,
    pub assigned_at: DateTime<Utc>,
    pub progress: Amount,
    #[serde(deserialize_with = "Option::deserialize")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub replaced_by_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub objective: Option<QuestObjective>,
}
/// Stored current-day allowance. Missing rows never imply an issued assignment.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DailyQuestAllowance {
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub generated_assignment_id: Option<Uuid>,
    pub rerolls_used: Amount,
    pub revision: Amount,
}
/// Bounded own assignment history plus the separately identified current UTC day.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    pub utc_day: String,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub daily: Option<DailyQuestAllowance>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<QuestAssignment>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl QuestPage {
    /// Check own identity, cursor, exact progress and explicitly available objective semantics.
    /// # Errors
    /// Rejects inconsistent identity, links, quantity, completion or paging metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner || !valid_day(&self.utc_day) {
            return Err("invalid quest owner/day");
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        if self.daily.as_ref().is_some_and(|daily| {
            daily.generated_assignment_id.is_some_and(|id| id.is_nil())
                || daily.rerolls_used > 1.into()
        }) {
            return Err("invalid daily quest allowance");
        }
        for item in &self.items {
            if item.definition_id.is_nil()
                || !valid_day(&item.source_day)
                || item
                    .replaced_by_id
                    .is_some_and(|id| id.is_nil() || id == item.id)
                || item.completed_at.is_some_and(|at| at < item.assigned_at)
                || item.replaced_by_id.is_some() && item.completed_at.is_some()
            {
                return Err("invalid assignment source");
            }
            if let Some(objective) = &item.objective
                && (objective.target.is_zero()
                    || ![500.into(), 750.into()].contains(&objective.reward_coins)
                    || item.progress > objective.target
                    || item.completed_at.is_some() != (item.progress == objective.target))
            {
                return Err("invalid qualified quest objective");
            }
        }
        Ok(())
    }
}
/// Current or prior season rating, without trusted scoring evidence.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub season_id: Uuid,
    pub tier: String,
    pub rating: Amount,
    pub revision: Amount,
    pub finalized: bool,
}
/// Exact caller-owned rank history with stable optional format/season filters.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RankPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<RankSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl RankPage {
    /// Check owner, immutable filter context, identifiers and page boundaries.
    /// # Errors
    /// Rejects foreign/filter-inconsistent or malformed rank rows and cursors.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
        format: Option<Uuid>,
        season: Option<Uuid>,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || self.items.iter().any(|item| {
                item.format_id.is_nil()
                    || item.season_id.is_nil()
                    || format.is_some_and(|id| id != item.format_id)
                    || season.is_some_and(|id| id != item.season_id)
            })
        {
            return Err("invalid owned rank projection");
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )
    }
}
/// The stored calendar state is source text; it does not claim rank finalization.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeasonSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub year: String,
    pub quarter: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub state: String,
}
/// Authenticated calendar metadata; not stock publication or a public leaderboard.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeasonPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<SeasonSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl SeasonPage {
    /// Verify the quarterly UTC calendar independently of any source state label.
    /// # Errors
    /// Rejects malformed identity, calendar ranges, unknown page shape or chronology.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner {
            return Err("invalid season reader");
        }
        for item in &self.items {
            let month = item.start_at.month();
            let expected_end = item.start_at.checked_add_months(chrono::Months::new(3));
            if item.year != item.start_at.year().to_string()
                || item.quarter != ((month - 1) / 3 + 1).to_string()
                || ![1, 4, 7, 10].contains(&month)
                || item.start_at.day() != 1
                || item.start_at.time() != chrono::NaiveTime::MIN
                || expected_end != Some(item.end_at)
            {
                return Err("invalid season calendar");
            }
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
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
    fn missing_nullable_fields_and_arrays_as_records_are_not_valid_pages() {
        let owner = Uuid::new_v4();
        let page = json!({"accountId":owner,"utcDay":"2026-11-04","daily":null,"items":[],"nextCursor":null});
        assert!(
            serde_json::from_value::<QuestPage>(page.clone())
                .unwrap()
                .validate(owner, None, 100)
                .is_ok()
        );
        for field in ["daily", "nextCursor"] {
            let mut missing = page.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<QuestPage>(missing).is_err());
        }
        let mut wrong = page;
        wrong["daily"] = json!([null, "0", "0"]);
        assert!(serde_json::from_value::<QuestPage>(wrong).is_err());
    }
    #[test]
    fn full_page_links_can_be_off_page_but_cursors_and_owner_must_match() {
        let owner = Uuid::new_v4();
        let item = Uuid::new_v4();
        let objective = json!({"definitionKey":"casts","revision":"v1","objectiveRef":"fixture:objective","target":"3","rewardCoins":"500"});
        let page = json!({"accountId":owner,"utcDay":"2026-11-04","daily":{"generatedAssignmentId":Uuid::new_v4(),"rerollsUsed":"1","revision":"9007199254740993"},"items":[{"id":item,"definitionId":Uuid::new_v4(),"sourceDay":"2026-11-03","assignedAt":"2026-11-03T01:00:00Z","progress":"0","completedAt":null,"replacedById":Uuid::new_v4(),"objective":objective}],"nextCursor":item});
        let parsed: QuestPage = serde_json::from_value(page.clone()).unwrap();
        assert!(parsed.validate(owner, None, 1).is_ok());
        assert!(parsed.validate(Uuid::new_v4(), None, 1).is_err());
        assert!(parsed.validate(owner, Some(item), 1).is_err());
        let mut over = page;
        over["items"][0]["progress"] = json!("4");
        assert!(
            serde_json::from_value::<QuestPage>(over)
                .unwrap()
                .validate(owner, None, 1)
                .is_err()
        );
    }
    #[test]
    fn rank_filter_and_calendar_rejection_do_not_invent_a_finalized_state() {
        let owner = Uuid::new_v4();
        let format = Uuid::new_v4();
        let season = Uuid::new_v4();
        let rank = json!({"accountId":owner,"items":[{"id":Uuid::new_v4(),"formatId":format,"seasonId":season,"tier":"bronze","rating":"9007199254740993","revision":"1","finalized":true}],"nextCursor":null});
        let parsed: RankPage = serde_json::from_value(rank).unwrap();
        assert!(
            parsed
                .validate(owner, None, 10, Some(format), Some(season))
                .is_ok()
        );
        assert!(
            parsed
                .validate(owner, None, 10, Some(Uuid::new_v4()), Some(season))
                .is_err()
        );
        let calendar = json!({"accountId":owner,"items":[{"id":season,"year":"2026","quarter":"4","startAt":"2026-10-01T00:00:00Z","endAt":"2027-01-01T00:00:00Z","state":"closed"}],"nextCursor":null});
        assert!(
            serde_json::from_value::<SeasonPage>(calendar.clone())
                .unwrap()
                .validate(owner, None, 10)
                .is_ok()
        );
        let mut wrong = calendar;
        wrong["items"][0]["endAt"] = json!("2027-01-02T00:00:00Z");
        assert!(
            serde_json::from_value::<SeasonPage>(wrong)
                .unwrap()
                .validate(owner, None, 10)
                .is_err()
        );
    }
}
