//! SQL-issued coordinates. Syntax and public cursor fields never confer authority.
use super::*;
use crate::player_wire::{PlayerCursor, PlayerHandle};

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
enum Purpose {
    Snapshot,
    History,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
enum Disclosure {
    Live,
    Completed,
    OperationalPrefix,
}
impl TryFrom<String> for Purpose {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "snapshot" => Ok(Self::Snapshot),
            "history" => Ok(Self::History),
            _ => Err("invalid cursor purpose"),
        }
    }
}
impl TryFrom<String> for Disclosure {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "live" => Ok(Self::Live),
            "completed" => Ok(Self::Completed),
            "operational-prefix" => Ok(Self::OperationalPrefix),
            _ => Err("invalid cursor disclosure"),
        }
    }
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    viewer_seat_id: Uuid,
    through_event_sequence: Amount,
    purpose: Purpose,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_receipt_id: Uuid,
    disclosure: Disclosure,
    token: PlayerHandle,
}
fn canonical_token(token: &PlayerHandle) -> bool {
    let Some(text) = token.as_str().strip_prefix("n1.") else {
        return false;
    };
    Uuid::parse_str(text).is_ok_and(|id| {
        id.get_version_num() == 4
            && id.get_variant() == uuid::Variant::RFC4122
            && id.to_string() == text
    })
}
impl Record {
    fn checked(self) -> Result<IssuedNativeCursor, NativeSourceError> {
        if self.version != "native-cursor-v1" || !canonical_token(&self.token) {
            return Err(invalid());
        }
        Ok(IssuedNativeCursor { record: self })
    }
    fn cursor(&self) -> PlayerCursor {
        PlayerCursor {
            basis: self.basis.clone(),
            viewer_seat_id: self.viewer_seat_id,
            through_event_sequence: self.through_event_sequence.clone(),
            token: self.token.clone(),
        }
    }
}
/// Complete independently read SQL cursor source, including private issuance context.
/// Cloning preserves issuer provenance; no Debug or serialization exposes that context.
#[derive(Clone)]
pub struct IssuedNativeCursor {
    record: Record,
}
impl IssuedNativeCursor {
    /// Public coordinate projection only, without receipt identity or disclosure flags.
    pub fn cursor(&self) -> PlayerCursor {
        self.record.cursor()
    }
    /// Verify lineage and progress against current participant access, never epoch order.
    /// # Errors
    /// Rejects foreign scope/viewer, non-live disclosure, a different native epoch,
    /// or a cursor beyond the actual accepted revision/event frontier.
    pub fn validate_current(&self, access: &NativeHumanAccess) -> Result<(), NativeSourceError> {
        let current = access.basis();
        let r = &self.record;
        if r.disclosure != Disclosure::Live
            || r.viewer_seat_id != access.viewer_seat_id()
            || r.basis.game_id != current.game_id
            || r.basis.context_id != current.context_id
            || r.basis.epoch != current.epoch
            || r.basis.revision > current.revision
            || &r.through_event_sequence > access.frontier()
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// Require a history-purpose cursor of this disclosure/viewer that does not
    /// pass the tip cut. Used where no live participant access applies.
    /// # Errors
    /// Rejects snapshot tokens and foreign viewer/epoch/disclosure/position.
    pub(super) fn validate_cut_history(
        &self,
        tip: &crate::player_wire::PlayerSnapshot,
        disclosure: NativeDisclosure,
    ) -> Result<(), NativeSourceError> {
        let r = &self.record;
        let b = &tip.basis;
        let expected = match disclosure {
            NativeDisclosure::LiveParticipant => Disclosure::Live,
            NativeDisclosure::CompletedParticipant => Disclosure::Completed,
        };
        if r.purpose != Purpose::History
            || r.disclosure != expected
            || r.viewer_seat_id != tip.viewer_seat_id
            || r.basis.game_id != b.game_id
            || r.basis.context_id != b.context_id
            || r.basis.epoch != b.epoch
            || r.basis.revision > b.revision
            || r.through_event_sequence > tip.through_event_sequence
        {
            return Err(invalid());
        }
        Ok(())
    }
    /// Require an actual history-purpose cursor under this human's current source.
    /// # Errors
    /// Rejects snapshot tokens as well as foreign/disclosure/epoch/frontier drift.
    pub fn validate_human_history(
        &self,
        access: &NativeHumanAccess,
    ) -> Result<(), NativeSourceError> {
        self.validate_current(access)?;
        if self.record.purpose != Purpose::History {
            return Err(invalid());
        }
        Ok(())
    }
}
impl NativeSourceRepository {
    /// Resolve a history coordinate; a valid snapshot token is still unavailable.
    /// # Errors
    /// Preserves strict issuer decoding and transport failures from read_human_cursor.
    pub fn read_human_history_cursor(
        &self,
        access: &NativeHumanAccess,
        requested: &PlayerCursor,
    ) -> Result<Option<IssuedNativeCursor>, NativeSourceError> {
        Ok(self
            .read_human_cursor(access, requested)?
            .filter(|issued| issued.validate_human_history(access).is_ok()))
    }
    /// Issue the SQL-owned position from an independently verified actual scan.
    /// SQL may reuse an earlier token for the same View/purpose/position even if
    /// after/limit differ. This is positional identity, not certification that a
    /// newly supplied scan replaced the original immutable issuance scan.
    /// # Errors
    /// Rejects scope/position/purpose/disclosure/source drift or bounded RPC faults.
    pub fn issue_human_history_cursor(
        &self,
        scan: &NativeArchiveScan,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<IssuedNativeCursor, NativeSourceError> {
        let cursor = self
            .cursor_record_bounded(
                "issue_native_cursor_source",
                &scan.cursor_arguments()?,
                Some(budget),
            )?
            .ok_or_else(invalid)?;
        let r = &cursor.record;
        if &r.basis != scan.basis()
            || r.viewer_seat_id != scan.viewer()
            || &r.through_event_sequence != scan.scanned_through()
            || r.purpose != Purpose::History
            || r.disclosure
                != match scan.disclosure() {
                    NativeDisclosure::LiveParticipant => Disclosure::Live,
                    NativeDisclosure::CompletedParticipant => Disclosure::Completed,
                }
            || r.source_receipt_id != scan.source_receipt()
        {
            return Err(invalid());
        }
        Ok(cursor)
    }
    /// Resolve a history token against the actual verified collection's perspective
    /// and disclosure. Completed perspectives come only from native/SQL completion,
    /// never the public cursor. Late original actor/source renewal remains required.
    /// # Errors
    /// Rejects malformed issuer objects or transport failures; unknown/foreign,
    /// snapshot-purpose or substituted scope returns None without a reset fallback.
    pub fn read_verified_history_cursor(
        &self,
        history: &NativeHumanHistory,
        requested: &PlayerCursor,
    ) -> Result<Option<IssuedNativeCursor>, NativeSourceError> {
        let source = history.cut_source();
        let tip = source.cuts.len().checked_sub(1).ok_or_else(invalid)?;
        self.read_cut_history_cursor(&source, tip, requested, None)
    }
    /// Resolve a history token against a verified cut source up to `at_index`
    /// (inclusive), for that source's perspective and disclosure only. The
    /// issued cursor must be anchored at the SQL-winning View of one of this
    /// source's cuts from the cursor's end through `at_index`. A supplied
    /// budget is charged for the record read.
    /// # Errors
    /// As read_verified_history_cursor, plus budget exhaustion.
    pub(super) fn read_cut_history_cursor<C: super::cuts::HistoryCut>(
        &self,
        source: &super::cuts::CutSource<'_, C>,
        at_index: usize,
        requested: &PlayerCursor,
        budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<Option<IssuedNativeCursor>, NativeSourceError> {
        let tip = source.cuts.get(at_index).ok_or_else(invalid)?.snapshot();
        let b = &tip.basis;
        if !canonical_token(&requested.token)
            || requested.viewer_seat_id != tip.viewer_seat_id
            || requested.basis.game_id != b.game_id
            || requested.basis.context_id != b.context_id
            || requested.basis.epoch != b.epoch
            || requested.basis.revision > b.revision
            || requested.through_event_sequence > tip.through_event_sequence
        {
            return Ok(None);
        }
        // The cursor must end on a cut at or before the requested coordinate.
        let Some(end) = source
            .index_of(&requested.through_event_sequence)
            .filter(|index| *index <= at_index)
        else {
            return Ok(None);
        };
        let issued = self.cursor_record_bounded(
            "read_native_cursor_source",
            &json!({
            "p_game":b.game_id,"p_viewer":tip.viewer_seat_id,"p_token":requested.token}),
            budget,
        )?;
        let expected = match source.disclosure {
            NativeDisclosure::LiveParticipant => Disclosure::Live,
            NativeDisclosure::CompletedParticipant => Disclosure::Completed,
        };
        // A scan is anchored at the View of its coordinate cut, so the issued
        // cursor's source receipt must be the View of a cut of this source
        // between the cursor's end and the requested coordinate.
        let anchored = |receipt: Uuid| {
            source.cuts[end..=at_index]
                .iter()
                .any(|cut| cut.view_receipt() == receipt)
        };
        Ok(issued.filter(|issued| {
            issued.cursor() == *requested
                && issued.record.purpose == Purpose::History
                && issued.record.disclosure == expected
                && anchored(issued.record.source_receipt_id)
        }))
    }
    /// Resolve an optional caller coordinate through SQL using the actual derived viewer.
    /// None means unknown/foreign/substituted/unusable cursor; it never requests fallback.
    /// The access observation must still be renewed before publication.
    /// # Errors
    /// Rejects malformed upstream objects, issuer identity/token grammar and backend faults.
    pub fn read_human_cursor(
        &self,
        access: &NativeHumanAccess,
        requested: &PlayerCursor,
    ) -> Result<Option<IssuedNativeCursor>, NativeSourceError> {
        let current = access.basis();
        if !canonical_token(&requested.token)
            || requested.viewer_seat_id != access.viewer_seat_id()
            || requested.basis.game_id != current.game_id
            || requested.basis.context_id != current.context_id
        {
            return Ok(None);
        }
        let issued = self.cursor_record(
            "read_native_cursor_source",
            &json!({
            "p_game":current.game_id,"p_viewer":access.viewer_seat_id(),"p_token":requested.token}),
        )?;
        match issued {
            Some(issued)
                if issued.cursor() == *requested && issued.validate_current(access).is_ok() =>
            {
                Ok(Some(issued))
            }
            _ => Ok(None),
        }
    }
    /// Issue a complete live snapshot coordinate from the actual SQL-winning private View.
    /// SQL alone creates/reuses its token. The public caller supplies no issuer operands.
    /// # Errors
    /// Rejects non-live Views, unavailable/foreign issuance, malformed upstream or backend IO.
    pub fn issue_human_snapshot_cursor(
        &self,
        issued: &IssuedNativeHumanView<'_>,
    ) -> Result<IssuedNativeCursor, NativeSourceError> {
        if issued.disclosure != NativeDisclosure::LiveParticipant {
            return Err(invalid());
        }
        let view = issued.view();
        let snapshot = view.snapshot();
        let viewer = view.scope().viewer_seat_id();
        let b = &snapshot.basis;
        let cursor = self
            .cursor_record(
                "issue_native_cursor_source",
                &json!({
            "p_game":b.game_id,"p_context":b.context_id,"p_epoch":b.epoch,"p_revision":b.revision,
            "p_viewer":viewer,"p_through_sequence":snapshot.through_event_sequence,
            "p_purpose":"snapshot","p_source_receipt":issued.receipt_id(),"p_scan":null}),
            )?
            .ok_or_else(invalid)?;
        let r = &cursor.record;
        if r.basis != *b
            || r.viewer_seat_id != viewer
            || r.through_event_sequence != snapshot.through_event_sequence
            || r.purpose != Purpose::Snapshot
            || r.disclosure != Disclosure::Live
            || r.source_receipt_id != issued.receipt_id()
        {
            return Err(invalid());
        }
        Ok(cursor)
    }
    fn cursor_record(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<Option<IssuedNativeCursor>, NativeSourceError> {
        self.cursor_record_bounded(name, args, None)
    }
    fn cursor_record_bounded(
        &self,
        name: &str,
        args: &serde_json::Value,
        budget: Option<&mut NativeRecoveryBudget>,
    ) -> Result<Option<IssuedNativeCursor>, NativeSourceError> {
        let response: ObjectResponse<Record> = self.native_record(name, args, budget)?;
        match response.0 {
            ApiResponse::Success { data } => data.checked().map(Some),
            ApiResponse::Fail { data }
                if data.len() == 1 && data.get("cursor").is_some_and(|v| v == "unavailable") =>
            {
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
}
#[cfg(test)]
mod tests;
