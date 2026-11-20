//! Declared stream records. There is no socket, publisher or permission-renewal runtime.
use super::*;
use crate::cosmetics::AppearanceSelection;
/// Select a participant stream and optionally resume from an independently issued cursor.
/// This request carries no viewer, disclosure, native artifact or authorization claim.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerStreamRequest {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub context_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub cursor: Option<PlayerCursor>,
}
impl PlayerStreamRequest {
    /// # Errors
    /// Rejects missing scope or a cursor differing from its independently read issuer
    /// record. Native epoch lineage and current access remain source-reader checks.
    pub fn validate(
        &self,
        viewer: Uuid,
        issued: Option<&PlayerCursor>,
    ) -> Result<(), InvalidPlayerWire> {
        PlayerViewRequest {
            game_id: self.game_id,
            context_id: self.context_id,
        }
        .validate()?;
        super::reconnect::validate_requested_cursor(
            self.cursor.as_ref(),
            issued,
            viewer,
            self.game_id,
            self.context_id,
        )
    }
}
/// Currently entitlement-resolved cosmetics, independent of immutable native state.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerSeatAppearance {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub appearance: AppearanceSelection,
}
/// Ordered complete seat metadata. A cosmetic change need not advance a game cursor.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayerPresentation {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub seats: Vec<PlayerSeatAppearance>,
}
impl PlayerPresentation {
    /// # Errors
    /// Rejects roster/order/identity mismatch or a selection differing from the
    /// current trusted resolver. IDs never establish entitlement by themselves.
    pub fn validate(&self, roster: &[Uuid], resolved: &Self) -> Result<(), InvalidPlayerWire> {
        if !(1..=6).contains(&roster.len())
            || roster.iter().any(Uuid::is_nil)
            || !super::actions::unique(roster.iter())
            || self != resolved
            || self
                .seats
                .iter()
                .map(|s| s.seat_id)
                .ne(roster.iter().copied())
            || self.seats.iter().any(|s| s.appearance.validate().is_err())
        {
            return Err(InvalidPlayerWire);
        }
        Ok(())
    }
}
/// Full accepted views are idempotent replacements. Gaps demand a fresh accepted
/// snapshot; the client must never manufacture private or missing event records.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PlayerStreamFrame {
    Snapshot {
        #[serde(deserialize_with = "crate::provider_json::object")]
        snapshot: Box<PlayerSnapshot>,
        #[serde(deserialize_with = "crate::provider_json::object")]
        cursor: PlayerCursor,
        #[serde(deserialize_with = "crate::provider_json::object")]
        presentation: PlayerPresentation,
    },
    Gap {
        #[serde(deserialize_with = "crate::provider_json::object")]
        cursor: PlayerCursor,
    },
}
/// Independently obtained current projection/issuer/resolver facts, or the actual
/// requested cursor for a gap. Neither variant is a serialized authorization claim.
pub enum PlayerStreamFacts<'a> {
    Snapshot {
        projection: PlayerProjectionFacts<'a>,
        issued_cursor: &'a PlayerCursor,
        presentation: &'a PlayerPresentation,
    },
    Gap {
        requested_cursor: &'a PlayerCursor,
    },
}
/// Validated delivery handling; duplicate board delivery still refreshes current
/// cosmetics, so entitlement revocation works while the native game is idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStreamEffect {
    ApplySnapshot,
    RefreshPresentation,
    RequireSnapshot,
}
impl PlayerStreamFrame {
    /// # Errors
    /// Rejects foreign or regressing frames, unissued cursors, private projection
    /// substitutions and stale presentation. The previous cursor must be independently
    /// verified and actually applied. No delivery loop or permission renewal is implied.
    pub fn validate(
        &self,
        previous: Option<&PlayerCursor>,
        facts: &PlayerStreamFacts<'_>,
    ) -> Result<PlayerStreamEffect, InvalidPlayerWire> {
        match (self, facts) {
            (
                Self::Snapshot {
                    snapshot,
                    cursor,
                    presentation,
                },
                PlayerStreamFacts::Snapshot {
                    projection,
                    issued_cursor,
                    presentation: resolved,
                },
            ) => {
                cursor.validate_snapshot(snapshot, projection, issued_cursor)?;
                presentation.validate(projection.roster, resolved)?;
                if let Some(previous) = previous {
                    cursor.validate_progress_from(previous)?;
                    if cursor == previous {
                        return Ok(PlayerStreamEffect::RefreshPresentation);
                    }
                }
                Ok(PlayerStreamEffect::ApplySnapshot)
            }
            (Self::Gap { cursor }, PlayerStreamFacts::Gap { requested_cursor }) => {
                cursor.validate(requested_cursor, requested_cursor.viewer_seat_id)?;
                if previous != Some(*requested_cursor) {
                    return Err(InvalidPlayerWire);
                }
                Ok(PlayerStreamEffect::RequireSnapshot)
            }
            _ => Err(InvalidPlayerWire),
        }
    }
}
