//! Sparse visible history uses the scanned frontier, not the last visible event.
use super::*;
/// Original read scope; a missing cursor is explicitly null.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerHistoryQuery {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub cursor: Option<PlayerCursor>,
    pub limit: NativeRevision,
}
impl PlayerHistoryQuery {
    /// # Errors
    /// Rejects foreign/unissued cursors, nil scope and a limit outside1..100.
    pub fn validate(
        &self,
        viewer: Uuid,
        issued: Option<&PlayerCursor>,
    ) -> Result<(), InvalidPlayerWire> {
        if viewer.is_nil()
            || self.game_id.is_nil()
            || self.context_id.is_nil()
            || !(1..=100).contains(&self.limit.get())
        {
            return Err(InvalidPlayerWire);
        }
        match (&self.cursor, issued) {
            (None, None) => Ok(()),
            (Some(cursor), Some(issued)) => {
                cursor.validate(issued, viewer)?;
                if cursor.basis.game_id != self.game_id
                    || cursor.basis.context_id != self.context_id
                {
                    return Err(InvalidPlayerWire);
                }
                Ok(())
            }
            _ => Err(InvalidPlayerWire),
        }
    }
}
/// Bounded visible observations and the full scanned-through coordinate.
/// Empty events may advance over any number of private/non-visible records.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerHistoryPage {
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub query: PlayerHistoryQuery,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub viewer_seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub events: Vec<PlayerVisibleEvent>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub scanned_through: PlayerCursor,
    pub has_more: bool,
}
/// Original cursor and actual historical scan/projector results, independently
/// supplied by the qualified reader. A cursor string is never issuance evidence.
pub struct PlayerHistoryFacts<'a> {
    pub viewer: Uuid,
    pub requested_cursor: Option<&'a PlayerCursor>,
    pub scanned_cursor: &'a PlayerCursor,
    pub has_more: bool,
    pub events: &'a [PlayerEventFacts<'a>],
}
impl PlayerHistoryPage {
    /// # Errors
    /// Rejects changed query/viewer, unknown cursors, repeated/decreasing/future
    /// records, regressing scan frontiers and substituted historical projections.
    /// It does not assert that no permitted observation was omitted by the reader.
    pub fn validate(
        &self,
        original: &PlayerHistoryQuery,
        f: &PlayerHistoryFacts<'_>,
    ) -> Result<(), InvalidPlayerWire> {
        original.validate(f.viewer, f.requested_cursor)?;
        self.scanned_through.validate(f.scanned_cursor, f.viewer)?;
        if &self.query != original
            || self.viewer_seat_id != f.viewer
            || self.scanned_through.basis.game_id != original.game_id
            || self.scanned_through.basis.context_id != original.context_id
            || self.has_more != f.has_more
            || self.events.len()
                > usize::try_from(original.limit.get()).map_err(|_| InvalidPlayerWire)?
            || self.events.len() != f.events.len()
            || original.cursor.as_ref().is_some_and(|c| {
                c.through_event_sequence > self.scanned_through.through_event_sequence
                    || c.basis
                        .compare_revision_in_epoch(&self.scanned_through.basis)
                        == Some(std::cmp::Ordering::Greater)
            })
        {
            return Err(InvalidPlayerWire);
        }
        let zero = crate::amount::Amount::from(0_u64);
        let mut previous = original
            .cursor
            .as_ref()
            .map_or(&zero, |c| &c.through_event_sequence);
        let mut previous_basis = original.cursor.as_ref().map(|c| &c.basis);
        if self.has_more && previous == &self.scanned_through.through_event_sequence {
            return Err(InvalidPlayerWire);
        }
        for (event, facts) in self.events.iter().zip(f.events) {
            if &event.event_sequence <= previous
                || event.event_sequence > self.scanned_through.through_event_sequence
                || event.basis.game_id != original.game_id
                || event.basis.context_id != original.context_id
                || event.viewer_seat_id != f.viewer
                || event
                    .basis
                    .compare_revision_in_epoch(&self.scanned_through.basis)
                    == Some(std::cmp::Ordering::Greater)
                || previous_basis.is_some_and(|basis| {
                    basis.compare_revision_in_epoch(&event.basis)
                        == Some(std::cmp::Ordering::Greater)
                })
            {
                return Err(InvalidPlayerWire);
            }
            event.validate(facts)?;
            previous = &event.event_sequence;
            previous_basis = Some(&event.basis);
        }
        Ok(())
    }
}
