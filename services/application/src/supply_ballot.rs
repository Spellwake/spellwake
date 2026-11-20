//! Frozen candidate and own approval contracts; trusted publication remains a private producer.
use crate::{amount::Amount, commands::CommandDisposition, progression::validate_page};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
/// Original ordered approvals; empty clears and retries preserve the original order.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceBallot {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub request_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub set_ids: Vec<Uuid>,
}
impl ReplaceBallot {
    /// # Errors
    /// Rejects absent request identity, repeated/nil approvals or more than ten choices.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.request_id.is_nil()
            || self.set_ids.len() > 10
            || self.set_ids.iter().any(Uuid::is_nil)
            || self.set_ids.iter().collect::<BTreeSet<_>>().len() != self.set_ids.len()
        {
            return Err("invalid original ballot approvals");
        }
        Ok(())
    }
}
impl ReplaceBallot {
    /// Bind original-season recovery while keeping outer request failures out of stored rejections.
    pub fn valid_disposition(&self, season: Uuid, disposition: &BallotDisposition) -> bool {
        match disposition {
            CommandDisposition::Accepted { ack } => ack.valid(season, self),
            CommandDisposition::Rejected { errors } => terminal_failure(errors),
            _ => true,
        }
    }
}
/// Approval count is the command's own accepted count, not the current ballot after later edits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BallotAck {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub resource_id: Uuid,
    pub approvals: Amount,
}
impl BallotAck {
    /// Check the original season and accepted approval count, preserving the original array order.
    pub fn valid(&self, season: Uuid, input: &ReplaceBallot) -> bool {
        self.resource_id == season && self.approvals == Amount::from(input.set_ids.len() as u64)
    }
}
/// Exact recovery of the original human command, independent of current cutoff/eligibility.
pub type BallotDisposition = CommandDisposition<BallotAck>;

pub(crate) fn terminal_failure(errors: &BTreeMap<String, String>) -> bool {
    errors.len() == 1
        && errors.iter().all(|(k, v)| {
            matches!(
                (k.as_str(), v.as_str()),
                ("account", "unavailable" | "qualifying real game required")
                    | ("ballot", "closed or unavailable")
                    | ("sets", "outside frozen candidates")
            )
        })
}
/// Only a month and aggregate count are public; account-list hashes remain private.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PopulationMonth {
    pub month: String,
    pub accounts: Amount,
}
/// Finalized public aggregates, distinct from unavailable publication.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BallotPublication {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub population: Vec<PopulationMonth>,
    pub population_sum: Amount,
    pub coefficient: Amount,
    pub quantity: Amount,
}
/// Candidate labels and own selection, with no live vote tally.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BallotCandidate {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub set_id: Uuid,
    pub name: String,
    pub approved: bool,
    #[serde(deserialize_with = "Option::deserialize")]
    pub final_approvals: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub winner: Option<bool>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub drop_quantity: Option<Amount>,
}
/// Paged frozen candidates plus separately bounded own approvals and published aggregates.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BallotPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub season_id: Uuid,
    pub cutoff: DateTime<Utc>,
    pub opened_at: DateTime<Utc>,
    pub is_open: bool,
    pub can_vote: bool,
    pub qualifies: bool,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    pub approved_set_ids: Vec<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub publication: Option<BallotPublication>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<BallotCandidate>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl BallotPage {
    /// # Errors
    /// Rejects foreign context, invented live tallies, malformed publication or inconsistent pages.
    pub fn validate(
        &self,
        owner: Uuid,
        season: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || self.season_id != season
            || self.opened_at >= self.cutoff
            || (self.can_vote && (!self.is_open || !self.qualifies))
            || self.approved_set_ids.len() > 10
            || self.approved_set_ids.iter().any(Uuid::is_nil)
            || self.approved_set_ids.windows(2).any(|p| p[0] >= p[1])
        {
            return Err("invalid ballot context");
        }
        validate_page(
            &self.items.iter().map(|r| r.set_id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for item in &self.items {
            if item.approved != self.approved_set_ids.contains(&item.set_id) {
                return Err("invalid frozen candidate");
            }
            if self.publication.is_none() {
                if item.final_approvals.is_some()
                    || item.winner.is_some()
                    || item.drop_quantity.is_some()
                {
                    return Err("unexpected unpublished tally");
                }
            } else if item.final_approvals.is_none()
                || item.winner.is_none()
                || (item.winner == Some(true)) != item.drop_quantity.is_some()
            {
                return Err("invalid finalized candidate");
            }
        }
        if let Some(publication) = &self.publication {
            if self.is_open
                || publication.population.len() != 3
                || publication.coefficient.is_zero()
                || publication.coefficient.to_string().len() > 9
            {
                return Err("invalid final publication");
            }
            let mut total = Amount::from(0);
            let mut previous = None;
            for row in &publication.population {
                let month = NaiveDate::parse_from_str(&format!("{}-01", row.month), "%Y-%m-%d")
                    .map_err(|_| "invalid population month")?;
                if row.month.len() != 7
                    || previous.is_some_and(|p: NaiveDate| {
                        p.checked_add_months(chrono::Months::new(1)) != Some(month)
                    })
                {
                    return Err("invalid population sequence");
                }
                total = total
                    .checked_add(&row.accounts)
                    .map_err(|_| "invalid population sum")?;
                previous = Some(month);
            }
            if total != publication.population_sum {
                return Err("invalid population total");
            }
        }
        Ok(())
    }
}
