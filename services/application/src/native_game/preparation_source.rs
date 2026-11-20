//! Private preparation observations, distinct from native qualification or Auth.
//! SQL owns canonical source hashes; no Rust JSON recanonicalization is attempted.
use crate::{
    ApiResponse,
    amount::Amount,
    backend::{BackendClient, BackendConfig, BackendError},
    decks::{DeckSection, PlayMode},
    match_history::ControllerKind,
    provider_json::ObjectResponse,
    rooms::configuration::MatchStructure,
    source_protocol::{SourceProof, parse_utc_microsecond, valid_digest},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeSet, error::Error, fmt, time::Duration};
use uuid::Uuid;
//222 bounds terms to2MiB. The status/sourceHash wrapper adds less than256bytes.
const MAX_PREPARATION_SOURCE_BYTES: u64 = 2 * 1024 * 1024 + 256;
mod between;
mod reserved;
pub use between::{
    NativeBetweenDecision, NativeBetweenMatch, NativeBetweenSource, NativeBetweenTerms,
};
pub use reserved::{NativeReservedGameMetadata, NativeReservedGameSource, NativeReservedSeat};

/// Safe preparation-boundary failure with private transport/codec causes retained.
pub struct NativePreparationError(Option<BackendError>);
impl fmt::Display for NativePreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("native preparation source is unavailable or inconsistent")
    }
}
impl fmt::Debug for NativePreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl Error for NativePreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.0.as_ref().map(|error| error as &(dyn Error + 'static))
    }
}
fn invalid() -> NativePreparationError {
    NativePreparationError(None)
}
fn backend(error: BackendError) -> NativePreparationError {
    NativePreparationError(Some(error))
}

/// Exact printing identity copied with a sealed selected entry, not latest art.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparationPrinting {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub external_printing_ref: SourceProof,
    pub finish: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub set_id: Uuid,
}
/// Shared222 selected-card projection, preserving nullable printing and quantity.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparationEntry {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub entry_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub definition_id: Uuid,
    pub oracle_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub definition_revision_id: Uuid,
    pub definition_revision_ref: SourceProof,
    pub rules_text_ref: SourceProof,
    pub program_ref: SourceProof,
    pub unlimited_basic_land: bool,
    #[serde(deserialize_with = "crate::provider_json::optional_object")]
    pub printing: Option<PreparationPrinting>,
    pub section: DeckSection,
    pub quantity: Amount,
}
/// Profile revision captured by the SQL source, without executable readiness.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparationProfile {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub key: String,
    pub revision_ref: SourceProof,
    pub rules_ref: SourceProof,
    #[serde(deserialize_with = "Option::deserialize")]
    pub planar_pool_ref: Option<SourceProof>,
    pub minimum_seats: Amount,
    pub seat_capacity: Amount,
    pub qualification_ref: SourceProof,
}
impl PreparationProfile {
    fn valid(&self) -> bool {
        self.minimum_seats >= Amount::from(2u64)
            && self.minimum_seats <= self.seat_capacity
            && self.seat_capacity <= Amount::from(6u64)
    }
}
fn valid_controller(kind: ControllerKind, account: Option<Uuid>, sponsor: Option<Uuid>) -> bool {
    match kind {
        ControllerKind::Human => account.is_some() && sponsor.is_none(),
        ControllerKind::Ai => account.is_none() && sponsor.is_some(),
    }
}
fn roster_size(count: usize) -> Option<Amount> {
    u64::try_from(count).ok().map(Amount::from)
}
/// Original selected release source references; SQL labels do not certify bytes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparationRelease {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub release_ref: SourceProof,
    pub kernel_ref: SourceProof,
    pub rules_ref: SourceProof,
    pub program_manifest_ref: SourceProof,
    pub evidence_ref: SourceProof,
}
/// Room observation may precede another participant selecting a deck.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(try_from = "String")]
pub enum PreparationStatus {
    Unselected,
    Unsupported,
    Available,
}
impl TryFrom<String> for PreparationStatus {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.as_str() {
            "unselected" => Ok(Self::Unselected),
            "unsupported" => Ok(Self::Unsupported),
            "available" => Ok(Self::Available),
            _ => Err("invalid preparation status"),
        }
    }
}
/// Occupied slot in canonical seat-index order. Sponsorship is not actor authority.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparationSlot {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub slot_id: Uuid,
    pub seat_index: Amount,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub account_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub sponsor_id: Option<Uuid>,
    pub controller_kind: ControllerKind,
    #[serde(deserialize_with = "Option::deserialize")]
    pub team_ref: Option<SourceProof>,
    pub ready: bool,
    pub source_status: PreparationStatus,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    pub selected_revision_id: Option<Uuid>,
    #[serde(deserialize_with = "crate::provider_json::optional_objects")]
    pub entries: Option<Vec<PreparationEntry>>,
}
/// Current room metadata at the exact requested revision.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparationRoom {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub id: Uuid,
    pub revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub host_id: Uuid,
    pub state: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub configuration_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub profile_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub format_id: Uuid,
    pub mode: PlayMode,
    pub competitive: bool,
    pub structure: MatchStructure,
    #[serde(deserialize_with = "Option::deserialize")]
    pub countdown_at: Option<String>,
}
/// Exact222 source terms, shared later by preparation and original execution.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeRoomTerms {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub room: PreparationRoom,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub profile: PreparationProfile,
    #[serde(deserialize_with = "crate::provider_json::object")]
    pub release: PreparationRelease,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    pub legality_revision_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pub slots: Vec<PreparationSlot>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RoomRecord {
    source_hash: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    terms: NativeRoomTerms,
}

pub(super) fn valid_entries(entries: &[PreparationEntry]) -> bool {
    let mut total = Amount::from(0u64);
    let maximum = Amount::from(1000u64);
    !entries.is_empty()
        && entries.len() <= 1000
        && entries
            .iter()
            .map(|entry| entry.entry_id)
            .collect::<BTreeSet<_>>()
            .len()
            == entries.len()
        && entries.iter().all(|entry| {
            if entry.quantity.is_zero() || entry.quantity > maximum {
                return false;
            }
            let Ok(next) = total.checked_add(&entry.quantity) else {
                return false;
            };
            total = next;
            true
        })
        && total <= maximum
}
impl RoomRecord {
    fn valid(&self, room: Uuid, basis: &Amount, release: Uuid, legality: Uuid) -> bool {
        let t = &self.terms;
        let p = &t.profile;
        valid_digest(&self.source_hash)
            && t.version == "native-room-source-v1"
            && t.room.id == room
            && t.room.revision == *basis
            && t.release.id == release
            && t.legality_revision_id == legality
            && t.room.profile_id == p.id
            && p.valid()
            && matches!(t.room.state.as_str(), "lobby" | "countdown")
            && (t.room.mode != PlayMode::Open || !t.room.competitive)
            && match (&t.room.countdown_at, t.room.state.as_str()) {
                (Some(time), "countdown") => parse_utc_microsecond(time).is_some(),
                (None, "lobby") => true,
                _ => false,
            }
            && !t.slots.is_empty()
            && t.slots.len() <= 6
            && roster_size(t.slots.len()).is_some_and(|count| count <= p.seat_capacity)
            && t.slots
                .iter()
                .map(|slot| slot.slot_id)
                .collect::<BTreeSet<_>>()
                .len()
                == t.slots.len()
            && t.slots
                .windows(2)
                .all(|pair| pair[0].seat_index < pair[1].seat_index)
            && t.slots.iter().all(|slot| {
                slot.seat_index < p.seat_capacity
                    && valid_controller(slot.controller_kind, slot.account_id, slot.sponsor_id)
                    && match (
                        &slot.source_status,
                        &slot.selected_revision_id,
                        &slot.entries,
                    ) {
                        (PreparationStatus::Unselected, None, None)
                        | (PreparationStatus::Unsupported, Some(_), None) => true,
                        (PreparationStatus::Available, Some(_), Some(entries)) => {
                            valid_entries(entries)
                        }
                        _ => false,
                    }
            })
    }
}
/// Sealed consistent observation. It is neither a user capability nor a source lease.
pub struct NativeRoomSource {
    record: RoomRecord,
}
impl NativeRoomSource {
    /// Preserve the exact PostgreSQL canonical source hash for late SQL revalidation.
    pub fn source_hash(&self) -> &str {
        &self.record.source_hash
    }
    /// Borrow copied terms without changing their printing or source identity.
    pub fn terms(&self) -> &NativeRoomTerms {
        &self.record.terms
    }
}
/// Trusted preparation-source transport. Never configure this in distributed clients.
pub struct NativePreparationRepository {
    backend: BackendClient,
}
impl NativePreparationRepository {
    pub(crate) fn new(backend: BackendClient) -> Self {
        Self { backend }
    }
    fn record<T: DeserializeOwned>(
        &self,
        name: &'static str,
        payload: Value,
        unavailable_field: &'static str,
    ) -> Result<Option<T>, NativePreparationError> {
        let response: ObjectResponse<T> = self
            .backend
            .rpc_bounded(name, &payload, MAX_PREPARATION_SOURCE_BYTES)
            .map_err(backend)?;
        match response.0 {
            ApiResponse::Success { data } => Ok(Some(data)),
            ApiResponse::Fail { data }
                if data.len() == 1
                    && data
                        .get(unavailable_field)
                        .is_some_and(|value| value == "unavailable") =>
            {
                Ok(None)
            }
            _ => Err(invalid()),
        }
    }
    /// Read trusted server configuration.
    /// # Errors
    /// Rejects missing/invalid server origin, key or transport configuration.
    pub fn from_env() -> Result<Self, NativePreparationError> {
        let config = BackendConfig::from_env().map_err(backend)?;
        Ok(Self::new(
            BackendClient::new(
                config,
                "spellwake-native-preparation/1",
                Duration::from_secs(60),
            )
            .map_err(backend)?,
        ))
    }
    pub(super) fn constructed_format_key(
        &self,
        format: Uuid,
    ) -> Result<String, NativePreparationError> {
        let bytes = self
            .backend
            .select_bytes_bounded(
                "format",
                &[
                    ("select", "id,key,name".to_owned()),
                    ("id", format!("eq.{format}")),
                ],
                4096,
            )
            .map_err(backend)?;
        let [row]: [crate::account::FormatSummary; 1] =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if row.id != format {
            return Err(invalid());
        }
        Ok(row.key)
    }
    /// Read an exact room revision before native deck qualification.
    /// # Errors
    /// Rejects nil identifiers before IO, malformed/duplicate/non-object payloads,
    /// mismatched scope, invalid slot/entry relationships or unknown failures.
    /// Only the declared source-unavailable result becomes None.
    pub fn room(
        &self,
        room: Uuid,
        basis: &Amount,
        release: Uuid,
        legality: Uuid,
    ) -> Result<Option<NativeRoomSource>, NativePreparationError> {
        if [room, release, legality].iter().any(Uuid::is_nil) {
            return Err(invalid());
        }
        let record: Option<RoomRecord> = self.record(
            "read_native_room_source",
            json!({"p_room":room,"p_basis":basis,"p_release":release,"p_legality":legality}),
            "source",
        )?;
        record
            .map(|record| {
                if record.valid(room, basis, release, legality) {
                    Ok(NativeRoomSource { record })
                } else {
                    Err(invalid())
                }
            })
            .transpose()
    }
}

#[cfg(test)]
mod live;
#[cfg(test)]
pub(in crate::native_game) mod tests;
