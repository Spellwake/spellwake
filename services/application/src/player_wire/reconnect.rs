//! Read-only reconnect composition. No transport or native handle issuer is installed.
use super::*;
use crate::amount::Amount;

/// Viewer-scoped scanned-through coordinate, including invisible events.
/// Opaque token syntax does not establish that a cursor was issued or is authentic.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerCursor {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub basis: PlayerBasis,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub viewer_seat_id: Uuid,
    pub through_event_sequence: Amount,
    pub token: PlayerHandle,
}
impl PlayerCursor {
    /// # Errors
    /// Rejects foreign/receding viewer coordinates. Cross-epoch lineage still
    /// requires the qualified issuer, since revisions in different epochs do not order.
    pub fn validate_progress_from(&self, previous: &Self) -> Result<(), InvalidPlayerWire> {
        if self.viewer_seat_id != previous.viewer_seat_id
            || self.basis.game_id != previous.basis.game_id
            || self.basis.context_id != previous.basis.context_id
            || self.through_event_sequence < previous.through_event_sequence
            || previous.basis.compare_revision_in_epoch(&self.basis)
                == Some(std::cmp::Ordering::Greater)
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
    /// Validate one accepted snapshot/cursor pair against independently established facts.
    /// # Errors
    /// Rejects an invalid projection or mismatched issued basis/viewer/scan frontier.
    pub fn validate_snapshot(
        &self,
        snapshot: &PlayerSnapshot,
        projection: &PlayerProjectionFacts<'_>,
        issued: &Self,
    ) -> Result<(), InvalidPlayerWire> {
        snapshot.validate(projection)?;
        self.validate(issued, projection.viewer)?;
        if self.basis != snapshot.basis
            || self.through_event_sequence != snapshot.through_event_sequence
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
    /// # Errors
    /// Rejects a substituted coordinate/token or a foreign viewer. The expected
    /// cursor must come from the actual verified issuer, never the request itself.
    pub fn validate(&self, issued: &Self, viewer: Uuid) -> Result<(), InvalidPlayerWire> {
        if viewer.is_nil()
            || self.viewer_seat_id != viewer
            || self != issued
            || self.basis.game_id.is_nil()
            || self.basis.context_id.is_nil()
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
}
/// Receipt lookup plus last known scan coordinate. Neither resubmits a move.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerReconnectRequest {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub receipts: PlayerReceiptQuery,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub cursor: Option<PlayerCursor>,
}
impl PlayerReconnectRequest {
    /// # Errors
    /// Rejects invalid intents or a cursor that differs from its verified issued
    /// record. Historical epoch lineage is verified by the source adapter.
    pub fn validate(
        &self,
        viewer: Uuid,
        issued: Option<&PlayerCursor>,
    ) -> Result<(), InvalidPlayerWire> {
        self.receipts.validate()?;
        validate_requested_cursor(
            self.cursor.as_ref(),
            issued,
            viewer,
            self.receipts.game_id,
            self.receipts.context_id,
        )
    }
}
/// Shared optional issued-cursor identity/scope check for reconnect and stream intake.
pub(super) fn validate_requested_cursor(
    requested: Option<&PlayerCursor>,
    issued: Option<&PlayerCursor>,
    viewer: Uuid,
    game: Uuid,
    context: Uuid,
) -> Result<(), InvalidPlayerWire> {
    if viewer.is_nil() {
        return Err(InvalidPlayerWire);
    }
    match (requested, issued) {
        (None, None) => Ok(()),
        (Some(cursor), Some(issued)) => {
            cursor.validate(issued, viewer)?;
            if cursor.basis.game_id != game || cursor.basis.context_id != context {
                return Err(InvalidPlayerWire);
            }
            Ok(())
        }
        _ => Err(InvalidPlayerWire),
    }
}

/// Independently observed snapshot and receipt facts, not an atomic combined read.
/// Current cosmetics remain a separate entitlement-resolved metadata projection.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerReconnect {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub snapshot: PlayerSnapshot,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub cursor: PlayerCursor,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub receipts: Vec<PlayerReceipt>,
}
/// Independently established source records. Copying request fields into these
/// facts does not establish authorization, cursor issuance or native lineage.
pub struct PlayerReconnectFacts<'a> {
    pub projection: PlayerProjectionFacts<'a>,
    pub requested_cursor: Option<&'a PlayerCursor>,
    pub snapshot_cursor: &'a PlayerCursor,
}
impl PlayerReconnect {
    /// # Errors
    /// Rejects substituted snapshot/frontier/viewer, foreign request targets,
    /// missing intent results and a snapshot older than the verified scan cursor.
    /// Historical receipts may precede the snapshot's revision and native epoch.
    pub fn validate(
        &self,
        request: &PlayerReconnectRequest,
        facts: &PlayerReconnectFacts<'_>,
    ) -> Result<(), InvalidPlayerWire> {
        request.validate(facts.projection.viewer, facts.requested_cursor)?;
        self.cursor
            .validate_snapshot(&self.snapshot, &facts.projection, facts.snapshot_cursor)?;
        if let Some(previous) = &request.cursor {
            self.cursor.validate_progress_from(previous)?;
        }
        if self.snapshot.basis.game_id != request.receipts.game_id
            || self.snapshot.basis.context_id != request.receipts.context_id
        {
            return Err(InvalidPlayerWire);
        }
        request
            .receipts
            .validate_receipts(&self.receipts, facts.projection.viewer)
    }
}
