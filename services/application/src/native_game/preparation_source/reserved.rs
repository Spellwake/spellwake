//! Reserved219 frontier before native initialization, never accepted world state.
use super::*;

/// Original admitted pool and selected next-game deck for one copied participant.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeReservedSeat {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub seat_id: Uuid,
    pub seat_index: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub account_id: Option<Uuid>,
    pub controller_kind: ControllerKind,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub sponsor_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub original_revision_id: Uuid,
    pub original_snapshot_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub selected_revision_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub selection_decision_id: Option<Uuid>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub selection_reply_ref: Option<SourceProof>,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub entries: Vec<PreparationEntry>,
    #[serde(default, deserialize_with = "crate::provider_json::objects")]
    pub custom_entries: Vec<PreparationCustomEntry>,
}
/// Exact reservation metadata. Historical creator/selection checks remain in SQL.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeReservedGameMetadata {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub room_id: Uuid,
    pub game_index: Amount,
    pub match_revision: Amount,
    pub mode: PlayMode,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub legality_revision_id: Uuid,
    pub competitive: bool,
    pub structure: MatchStructure,
    pub admission_qualification_ref: SourceProof,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub profile: PreparationProfile,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub release: PreparationRelease,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub seats: Vec<NativeReservedSeat>,
    #[serde(default)]
    pub allow_custom_cards: bool,
    #[serde(default)]
    pub share_host_decks: bool,
}
impl NativeReservedGameMetadata {
    fn valid(&self, game: Uuid) -> bool {
        let allow_custom =
            self.allow_custom_cards && self.mode == PlayMode::Open && !self.competitive;
        matches!(
            self.version.as_str(),
            "native-admission-source-v1" | "native-admission-source-v2"
        ) && self.game_id == game
            && self.profile.valid()
            && (self.mode != PlayMode::Open || !self.competitive)
            && (!(self.allow_custom_cards || self.share_host_decks)
                || (self.mode == PlayMode::Open && !self.competitive))
            && (2..=6).contains(&self.seats.len())
            && roster_size(self.seats.len()).is_some_and(|count| {
                count >= self.profile.minimum_seats && count <= self.profile.seat_capacity
            })
            && (self.structure != MatchStructure::Bo1 || self.game_index.is_zero())
            && self
                .seats
                .iter()
                .map(|seat| seat.seat_id)
                .collect::<BTreeSet<_>>()
                .len()
                == self.seats.len()
            && self
                .seats
                .windows(2)
                .all(|pair| pair[0].seat_index < pair[1].seat_index)
            && self.seats.iter().all(|seat| {
                seat.seat_index < self.profile.seat_capacity
                    && valid_controller(seat.controller_kind, seat.account_id, seat.sponsor_id)
                    && (seat.custom_entries.is_empty() || allow_custom)
                    && valid_all_entries(&seat.entries, &seat.custom_entries)
                    && if self.game_index.is_zero() {
                        seat.selected_revision_id == seat.original_revision_id
                            && seat.selection_decision_id.is_none()
                            && seat.selection_reply_ref.is_none()
                    } else {
                        seat.selection_decision_id.is_some() && seat.selection_reply_ref.is_some()
                    }
            })
    }
}
/// Sealed observation of an unopened reservation. It grants no player authority.
pub struct NativeReservedGameSource {
    record: NativeReservedGameMetadata,
}
impl NativeReservedGameSource {
    /// Borrow original and selected deck identities in copied seat order.
    pub fn metadata(&self) -> &NativeReservedGameMetadata {
        &self.record
    }
}
impl NativePreparationRepository {
    /// Observe an exact reserved game before native initialization/acceptance.
    /// # Errors
    /// Rejects nil ID, malformed/duplicate/non-object or over-budget records,
    /// unknown failure vocabulary, inconsistent roster or selection coordinates.
    /// Draws may produce game indices above three; no BO3 game-count cap is added.
    pub fn reserved_game(
        &self,
        game: Uuid,
    ) -> Result<Option<NativeReservedGameSource>, NativePreparationError> {
        if game.is_nil() {
            return Err(invalid());
        }
        let record: Option<NativeReservedGameMetadata> = self.record(
            "read_native_admission_source",
            json!({"p_game":game}),
            "game",
        )?;
        record
            .map(|record| {
                if record.valid(game) {
                    Ok(NativeReservedGameSource { record })
                } else {
                    Err(invalid())
                }
            })
            .transpose()
    }
}
