//! Participant scalar history. Native views, permitted actions and replay payloads are separate.
use crate::{
    account::PublicAccountIdentity, amount::Amount, decks::PlayMode, progression::validate_page,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;
/// Accepted match phase, without implying a running native host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum MatchPhase {
    Admitting,
    Ready,
    Active,
    BetweenGames,
    Completed,
    NoContest,
}
impl TryFrom<String> for MatchPhase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "admitting" => Ok(Self::Admitting),
            "ready" => Ok(Self::Ready),
            "active" => Ok(Self::Active),
            "between_games" => Ok(Self::BetweenGames),
            "completed" => Ok(Self::Completed),
            "no_contest" => Ok(Self::NoContest),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Persisted game phase; a reservation is not initialized gameplay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum GamePhase {
    Canceled,
    Reserved,
    Active,
    Completed,
    NoContest,
}
impl TryFrom<String> for GamePhase {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "canceled" => Ok(Self::Canceled),
            "reserved" => Ok(Self::Reserved),
            "active" => Ok(Self::Active),
            "completed" => Ok(Self::Completed),
            "no_contest" => Ok(Self::NoContest),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Accepted operational availability, independent of match scoring phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum Availability {
    Available,
    Recovering,
    Unrecoverable,
}
impl TryFrom<String> for Availability {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "available" => Ok(Self::Available),
            "recovering" => Ok(Self::Recovering),
            "unrecoverable" => Ok(Self::Unrecoverable),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Normalized result outcome; missing historical winners remain distinct from a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum Outcome {
    Win,
    Draw,
    NoContest,
}
impl TryFrom<String> for Outcome {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "win" => Ok(Self::Win),
            "draw" => Ok(Self::Draw),
            "no_contest" => Ok(Self::NoContest),
            _ => Err("invalid string enum value"),
        }
    }
}

/// A seat's original controller class, not its sponsor's human identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum ControllerKind {
    Human,
    Ai,
}
impl TryFrom<String> for ControllerKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "human" => Ok(Self::Human),
            "ai" => Ok(Self::Ai),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Only answered/pending/canceled decisions exist in the current canonical source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum DecisionState {
    Pending,
    Answered,
    Canceled,
}
impl TryFrom<String> for DecisionState {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "pending" => Ok(Self::Pending),
            "answered" => Ok(Self::Answered),
            "canceled" => Ok(Self::Canceled),
            _ => Err("invalid string enum value"),
        }
    }
}

/// A terminal result's source class, without any private source identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum TerminalKind {
    BankForfeit,
    GameOutcome,
    UnrecoverableIncident,
}
impl TryFrom<String> for TerminalKind {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "bank_forfeit" => Ok(Self::BankForfeit),
            "game_outcome" => Ok(Self::GameOutcome),
            "unrecoverable_incident" => Ok(Self::UnrecoverableIncident),
            _ => Err("invalid string enum value"),
        }
    }
}

/// Admitted participant metadata, without release/artifact/deck source locators.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    pub structure: String,
    pub state: MatchPhase,
    pub revision: Amount,
    pub started_at: DateTime<Utc>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub current_game_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub reserved_game_id: Option<Uuid>,
    pub availability: Availability,
    pub availability_revision: Amount,
}
/// Terminal scalar result; game history stays paged rather than embedding its manifest.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchResult {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub outcome: Outcome,
    pub committed_at: DateTime<Utc>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub ranking_format_id: Option<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub rank_sequence: Option<Amount>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid_list")]
    pub winner_seat_ids: Option<Vec<Uuid>>,
    pub terminal_kind: TerminalKind,
}
/// Original caller and target are checked independently of any paged roster.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchDetail {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub r#match: MatchSummary,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub own_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub result: Option<MatchResult>,
}
/// Per-game normalized outcome, omitting event/proof/private payload references.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameResult {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub outcome: Outcome,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid_list")]
    pub winner_seat_ids: Option<Vec<Uuid>>,
    pub end_reason: String,
}
/// One game header plus nullable actual committed result.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    /// Immutable game-context locator, absent before a context exists; not readiness.
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub context_id: Option<Uuid>,
    pub game_index: Amount,
    pub state: GamePhase,
    pub accepted_revision: Amount,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub result: Option<GameResult>,
}
/// Accepted remaining clock metadata; reads never advance or refill it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClockSummary {
    pub remaining_ms: Amount,
    pub revision: Amount,
    pub policy_ref: String,
    pub paused: bool,
    pub last_accounted_at: DateTime<Utc>,
}
/// Human display identity or explicit AI absence, without sponsor or copied deck.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeatSummary {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub seat_index: Amount,
    pub controller_kind: ControllerKind,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub person: Option<PublicAccountIdentity>,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub clock: Option<ClockSummary>,
}
/// The caller’s decision metadata; reply/qualification sources remain private.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnDecision {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    pub decision_kind: String,
    pub basis_revision: Amount,
    #[serde(deserialize_with = "Option::deserialize")]
    pub deadline_at: Option<DateTime<Utc>>,
    pub clock_revision: Amount,
    pub state: DecisionState,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub after_game_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub selected_deck_revision_id: Option<Uuid>,
    pub opened_at: DateTime<Utc>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub deadline_policy_ref: Option<String>,
    pub deadline_revision: Amount,
    #[serde(deserialize_with = "Option::deserialize")]
    pub deadline_availability_revision: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub deadline_remaining_us: Option<Amount>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub deadline_accounted_at: Option<DateTime<Utc>>,
}
/// Bounded ascending UUID page with explicit original caller.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchList {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<MatchSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl MatchList {
    /// # Errors
    /// Rejects foreign context, malformed rows or inconsistent exclusive keyset metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil() || self.account_id != owner {
            return Err("invalid history context");
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}
/// Bounded ascending UUID page with explicit original caller and match context.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GamePage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<GameSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl GamePage {
    /// # Errors
    /// Rejects foreign context, malformed rows or inconsistent exclusive keyset metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        matched: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || matched.is_nil()
            || self.match_id != matched
        {
            return Err("invalid history context");
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        let contexts: Vec<_> = self
            .items
            .iter()
            .filter_map(|item| item.context_id)
            .collect();
        if contexts.iter().collect::<BTreeSet<_>>().len() != contexts.len() {
            return Err("repeated game context");
        }
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}
/// Bounded ascending UUID page with explicit original caller and match context.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeatPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<SeatSummary>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl SeatPage {
    /// # Errors
    /// Rejects foreign context, malformed rows or inconsistent exclusive keyset metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        matched: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || matched.is_nil()
            || self.match_id != matched
        {
            return Err("invalid history context");
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}
/// Bounded ascending UUID page with explicit original caller and match context.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionPage {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub account_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub items: Vec<OwnDecision>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub next_cursor: Option<Uuid>,
}
impl DecisionPage {
    /// # Errors
    /// Rejects foreign context, malformed rows or inconsistent exclusive keyset metadata.
    pub fn validate(
        &self,
        owner: Uuid,
        matched: Uuid,
        after: Option<Uuid>,
        limit: usize,
    ) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || matched.is_nil()
            || self.match_id != matched
        {
            return Err("invalid history context");
        }
        validate_page(
            &self.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            after,
            limit,
            self.next_cursor,
        )?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

fn source_text(value: &str, max: usize) -> bool {
    !value.contains('\0') && (1..=max).contains(&value.trim_matches(' ').chars().count())
}
fn validate_winners(outcome: Outcome, winners: &Option<Vec<Uuid>>) -> Result<(), &'static str> {
    if let Some(ids) = winners
        && (ids.len() > 6
            || ids.iter().any(Uuid::is_nil)
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
            || ((outcome == Outcome::Win) != !ids.is_empty()))
    {
        return Err("invalid normalized winners");
    }
    Ok(())
}
impl MatchSummary {
    fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_nil()
            || self.format_id.is_nil()
            || self.profile_id.is_nil()
            || (self.competitive && self.mode != PlayMode::Collection)
            || !matches!(self.structure.as_str(), "bo1" | "bo3")
            || (self.current_game_id.is_some() && self.reserved_game_id.is_some())
        {
            return Err("invalid match metadata");
        }
        Ok(())
    }
}
impl MatchDetail {
    /// # Errors
    /// Rejects another account/target, malformed source metadata or a result preceding admission.
    pub fn validate(&self, owner: Uuid, matched: Uuid) -> Result<(), &'static str> {
        if owner.is_nil()
            || self.account_id != owner
            || self.r#match.id != matched
            || self.own_seat_id.is_nil()
        {
            return Err("invalid match context");
        }
        self.r#match.validate()?;
        if let Some(result) = &self.result {
            if result.id.is_nil()
                || result.committed_at < self.r#match.started_at
                || result.ranking_format_id.is_some() != result.rank_sequence.is_some()
            {
                return Err("invalid terminal result");
            }
            validate_winners(result.outcome, &result.winner_seat_ids)?;
        }
        Ok(())
    }
}
impl GameSummary {
    fn validate(&self) -> Result<(), &'static str> {
        if self.context_id.is_some_and(|id| id.is_nil()) {
            return Err("invalid game context");
        }
        if let Some(result) = &self.result {
            if result.id.is_nil() || !source_text(&result.end_reason, 2048) {
                return Err("invalid game outcome");
            }
            validate_winners(result.outcome, &result.winner_seat_ids)?;
        }
        Ok(())
    }
}
impl SeatSummary {
    fn validate(&self) -> Result<(), &'static str> {
        if self.seat_index >= 6.into()
            || (self.controller_kind == ControllerKind::Human) != self.person.is_some()
        {
            return Err("invalid match seat");
        }
        if let Some(person) = &self.person {
            person.validate()?;
        }
        if let Some(clock) = &self.clock
            && (!source_text(&clock.policy_ref, 2048))
        {
            return Err("invalid clock policy");
        }
        Ok(())
    }
}
impl OwnDecision {
    fn validate(&self) -> Result<(), &'static str> {
        if self.seat_id.is_nil() || !source_text(&self.decision_kind, 2048) {
            return Err("invalid own decision");
        }
        if self.deadline_policy_ref.is_none() {
            if !self.deadline_revision.is_zero()
                || self.deadline_at.is_some()
                || self.deadline_availability_revision.is_some()
                || self.deadline_remaining_us.is_some()
                || self.deadline_accounted_at.is_some()
            {
                return Err("invalid inactive deadline");
            }
        } else if self.deadline_revision.is_zero()
            || self.deadline_availability_revision.is_none()
            || self.deadline_accounted_at.is_none()
            || self.deadline_at.is_some() == self.deadline_remaining_us.is_some()
            || self
                .deadline_policy_ref
                .as_ref()
                .is_some_and(|s| !source_text(s, 2048))
        {
            return Err("invalid qualified deadline metadata");
        }
        Ok(())
    }
}

#[cfg(test)]
mod string_enum_contract_tests {
    use super::*;
    #[test]
    fn canonical_string_protocol() {
        crate::test_http::string_enum_roundtrip::<MatchPhase>(&[
            "admitting",
            "ready",
            "active",
            "between_games",
            "completed",
            "no_contest",
        ]);
        crate::test_http::string_enum_roundtrip::<GamePhase>(&[
            "canceled",
            "reserved",
            "active",
            "completed",
            "no_contest",
        ]);
        crate::test_http::string_enum_roundtrip::<Availability>(&[
            "available",
            "recovering",
            "unrecoverable",
        ]);
        crate::test_http::string_enum_roundtrip::<Outcome>(&["win", "draw", "no_contest"]);
        crate::test_http::string_enum_roundtrip::<ControllerKind>(&["human", "ai"]);
        crate::test_http::string_enum_roundtrip::<DecisionState>(&[
            "pending", "answered", "canceled",
        ]);
        crate::test_http::string_enum_roundtrip::<TerminalKind>(&[
            "bank_forfeit",
            "game_outcome",
            "unrecoverable_incident",
        ]);
    }
}
