//! Participant replay envelopes, not a private archive reader or native replay codec.
use super::*;
use crate::amount::Amount;
/// Public codec version label. It is never a private schema proof or storage locator.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PlayerReplaySchema(String);
impl TryFrom<String> for PlayerReplaySchema {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        if s.is_empty()
            || s.len() > 128
            || !s
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || !s.as_bytes()[0].is_ascii_alphanumeric()
        {
            return Err(InvalidPlayerWire);
        }
        Ok(Self(s))
    }
}
impl From<PlayerReplaySchema> for String {
    fn from(value: PlayerReplaySchema) -> Self {
        value.0
    }
}
/// Requested mode alone never establishes participant or completion authority.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum PlayerReplayMode {
    HistoricalSeat,
    CompletedFullInformation,
}
impl TryFrom<String> for PlayerReplayMode {
    type Error = InvalidPlayerWire;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "historical-seat" => Ok(Self::HistoricalSeat),
            "completed-full-information" => Ok(Self::CompletedFullInformation),
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Board at one exact historical coordinate plus a bounded history-page request.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerReplayRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
    pub at_event_sequence: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub perspective_seat_id: Uuid,
    pub mode: PlayerReplayMode,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub history: PlayerHistoryQuery,
}
impl PlayerReplayRequest {
    /// # Errors
    /// Rejects malformed scope, substituted/future history cursors and foreign viewers.
    /// Does not authenticate the requester or authorize full-information access.
    pub fn validate(&self, issued: Option<&PlayerCursor>) -> Result<(), InvalidPlayerWire> {
        self.history.validate(self.perspective_seat_id, issued)?;
        if self.game_id.is_nil()
            || self.context_id.is_nil()
            || self.history.game_id != self.game_id
            || self.history.context_id != self.context_id
            || self
                .history
                .cursor
                .as_ref()
                .is_some_and(|c| c.through_event_sequence > self.at_event_sequence)
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
}
/// Safe historical presentation without artifact URLs, source proofs or native state.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerReplayFrame {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub request: PlayerReplayRequest,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub release_id: Uuid,
    pub schema_revision: PlayerReplaySchema,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub snapshot: Box<PlayerSnapshot>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub history: PlayerHistoryPage,
}
/// Original archive/context lineage resolved by the qualified reader, not a
/// deserializable proof object. Native codec and artifact verification remain required.
pub struct PlayerReplaySource<'a> {
    pub game_id: Uuid,
    pub context_id: Uuid,
    pub release_id: Uuid,
    pub schema_revision: &'a PlayerReplaySchema,
}
/// Actual session account and retained participant/seat provenance are independent
/// of the requested perspective. Completion/eligibility come from authoritative state.
pub struct PlayerReplayFacts<'a> {
    pub requester_account_id: Uuid,
    pub participant_account_ids: &'a [Uuid],
    pub historical_seat_id: Uuid,
    pub historical_seat_owner: Option<Uuid>,
    pub completed: bool,
    pub full_information_eligible: bool,
    pub source: PlayerReplaySource<'a>,
    pub projection: PlayerProjectionFacts<'a>,
    pub history: PlayerHistoryFacts<'a>,
}
impl PlayerReplayFrame {
    /// # Errors
    /// Rejects nonparticipants, another player's active-game perspective, premature
    /// full disclosure, current-release/schema substitution, foreign board/history,
    /// or any data beyond the exact requested coordinate. The qualified reader must
    /// still establish native replay, historical visibility and original artifact lineage.
    pub fn validate(
        &self,
        original: &PlayerReplayRequest,
        f: &PlayerReplayFacts<'_>,
    ) -> Result<(), InvalidPlayerWire> {
        original.validate(f.history.requested_cursor)?;
        let full = original.mode == PlayerReplayMode::CompletedFullInformation;
        if f.requester_account_id.is_nil()
            || f.participant_account_ids.len() > 6
            || f.participant_account_ids.iter().any(Uuid::is_nil)
            || !super::actions::unique(f.participant_account_ids.iter())
            || !f.participant_account_ids.contains(&f.requester_account_id)
            || f.historical_seat_id != original.perspective_seat_id
            || if full {
                !f.completed || !f.full_information_eligible
            } else {
                f.historical_seat_owner != Some(f.requester_account_id)
            }
            || &self.request != original
            || f.source.game_id != original.game_id
            || f.source.context_id != original.context_id
            || f.source.release_id.is_nil()
            || self.release_id != f.source.release_id
            || &self.schema_revision != f.source.schema_revision
            || self.snapshot.basis.game_id != original.game_id
            || self.snapshot.basis.context_id != original.context_id
            || self.snapshot.viewer_seat_id != original.perspective_seat_id
            || self.snapshot.through_event_sequence != original.at_event_sequence
            || f.projection.authorized_full_disclosure != full
            || f.history.viewer != original.perspective_seat_id
            || self.history.scanned_through.through_event_sequence > original.at_event_sequence
            || (self.history.has_more
                && self.history.scanned_through.through_event_sequence
                    == original.at_event_sequence)
            || self
                .history
                .scanned_through
                .basis
                .compare_revision_in_epoch(&self.snapshot.basis)
                == Some(std::cmp::Ordering::Greater)
            || f.history.events.iter().any(|e| {
                e.before_projection.authorized_full_disclosure != full
                    || e.after_projection.authorized_full_disclosure != full
                    || e.before_projection.roster != f.projection.roster
                    || e.after_projection.roster != f.projection.roster
            })
        {
            return Err(InvalidPlayerWire);
        }
        self.snapshot.validate(&f.projection)?;
        self.history.validate(&original.history, &f.history)
    }
}
