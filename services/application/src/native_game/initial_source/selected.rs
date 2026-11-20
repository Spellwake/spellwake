//! Private selected-entry projection, immutable physical origin and strict joins.
use super::*;
use crate::{
    amount::Amount,
    native_game::preparation_source::{
        NativeReservedGameSource, PreparationEntry, PreparationPrinting, valid_entries,
    },
    source_protocol::SourceProof,
};
use serde::{Serializer, ser::SerializeStruct};
use spellwake_engine::object_identity::{CardId, GameObject};
use std::collections::BTreeMap;

/// Explicit trusted selected-entry assignment in original participant/card order.
/// Repeated entry IDs are valid within quantity. This does not select zone policy.
pub struct NativeInitialSelection {
    pub seat_id: Uuid,
    pub entry_ids: Vec<Uuid>,
}

// SourceProof deliberately remains nonserializable. Only this private Initial
// projection serializes its original strings; it is never a public API response.
fn proof<S: Serializer>(value: &SourceProof, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(value.as_str())
}
fn nullable_proof<S: Serializer>(
    value: &Option<SourceProof>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serializer.serialize_some(value.as_str()),
        None => serializer.serialize_none(),
    }
}
fn copy_proof(value: &SourceProof) -> Result<SourceProof, NativeInitialError> {
    value
        .as_str()
        .parse()
        .map_err(|_| NativeInitialError::Invalid)
}
struct PrintingRef<'a>(&'a PreparationPrinting);
impl Serialize for PrintingRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let PreparationPrinting {
            id,
            external_printing_ref,
            finish,
            set_id,
        } = self.0;
        let mut record = serializer.serialize_struct("PreparationPrinting", 4)?;
        record.serialize_field("id", id)?;
        record.serialize_field("externalPrintingRef", external_printing_ref.as_str())?;
        record.serialize_field("finish", finish)?;
        record.serialize_field("setId", set_id)?;
        record.end()
    }
}
struct EntryRef<'a>(&'a PreparationEntry);
impl Serialize for EntryRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Exhaustive destructuring makes new maintained projection fields a compile
        // error here rather than silently dropping them from retained provenance.
        let PreparationEntry {
            entry_id,
            definition_id,
            oracle_ref,
            definition_revision_id,
            definition_revision_ref,
            rules_text_ref,
            program_ref,
            unlimited_basic_land,
            printing,
            section,
            quantity,
        } = self.0;
        let mut record = serializer.serialize_struct("PreparationEntry", 11)?;
        record.serialize_field("entryId", entry_id)?;
        record.serialize_field("definitionId", definition_id)?;
        record.serialize_field("oracleRef", oracle_ref.as_str())?;
        record.serialize_field("definitionRevisionId", definition_revision_id)?;
        record.serialize_field("definitionRevisionRef", definition_revision_ref.as_str())?;
        record.serialize_field("rulesTextRef", rules_text_ref.as_str())?;
        record.serialize_field("programRef", program_ref.as_str())?;
        record.serialize_field("unlimitedBasicLand", unlimited_basic_land)?;
        record.serialize_field("printing", &printing.as_ref().map(PrintingRef))?;
        record.serialize_field("section", section)?;
        record.serialize_field("quantity", quantity)?;
        record.end()
    }
}
fn entries<S: Serializer>(values: &[PreparationEntry], serializer: S) -> Result<S::Ok, S::Error> {
    values
        .iter()
        .map(EntryRef)
        .collect::<Vec<_>>()
        .serialize(serializer)
}
fn copy_entry(entry: &PreparationEntry) -> Result<PreparationEntry, NativeInitialError> {
    // Use the maintained strict decoder over this exact private projection. This
    // is an ownership copy, not SQL JSONB canonicalization or a source fingerprint.
    let bytes = serde_json::to_vec(&EntryRef(entry)).map_err(NativeInitialError::Json)?;
    crate::decode_wire_object(&bytes).map_err(NativeInitialError::Json)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SelectedSeat {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    original_revision_id: Uuid,
    #[serde(serialize_with = "proof")]
    original_snapshot_ref: SourceProof,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    selected_revision_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::nullable_uuid")]
    selection_decision_id: Option<Uuid>,
    #[serde(
        serialize_with = "nullable_proof",
        deserialize_with = "Option::deserialize"
    )]
    selection_reply_ref: Option<SourceProof>,
    #[serde(
        serialize_with = "entries",
        deserialize_with = "crate::provider_json::objects"
    )]
    entries: Vec<PreparationEntry>,
    #[serde(deserialize_with = "crate::identifiers::uuid_list")]
    entry_ids: Vec<Uuid>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SelectedRoot {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    match_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    profile_id: Uuid,
    game_index: Amount,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    seats: Vec<SelectedSeat>,
}
impl SelectedRoot {
    pub(super) fn from_reserved(
        source: &NativeReservedGameSource,
        selection: Vec<NativeInitialSelection>,
    ) -> Result<Self, NativeInitialError> {
        let r = source.metadata();
        if selection.len() != r.seats.len() {
            return Err(NativeInitialError::ForeignSource);
        }
        let seats = r
            .seats
            .iter()
            .zip(selection)
            .map(|(seat, selected)| {
                if seat.seat_id != selected.seat_id {
                    return Err(NativeInitialError::ForeignSource);
                }
                Ok(SelectedSeat {
                    seat_id: seat.seat_id,
                    original_revision_id: seat.original_revision_id,
                    original_snapshot_ref: copy_proof(&seat.original_snapshot_ref)?,
                    selected_revision_id: seat.selected_revision_id,
                    selection_decision_id: seat.selection_decision_id,
                    selection_reply_ref: seat
                        .selection_reply_ref
                        .as_ref()
                        .map(copy_proof)
                        .transpose()?,
                    entries: seat
                        .entries
                        .iter()
                        .map(copy_entry)
                        .collect::<Result<_, _>>()?,
                    entry_ids: selected.entry_ids,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            game_id: r.game_id,
            match_id: r.match_id,
            release_id: r.release.id,
            profile_id: r.profile.id,
            game_index: r.game_index.clone(),
            seats,
        })
    }
    pub(super) fn validate(
        &self,
        ids: NativeInitialIds,
        participants: &[NativeInitialParticipant],
    ) -> Result<Vec<Vec<usize>>, NativeInitialError> {
        if self.game_id != ids.game_id
            || self.release_id != ids.release_id
            || self.profile_id != ids.profile_id
            || self.match_id.is_nil()
            || self.seats.len() != participants.len()
        {
            return Err(NativeInitialError::ForeignSource);
        }
        self.seats
            .iter()
            .zip(participants)
            .map(|(seat, participant)| {
                if seat.seat_id != participant.seat_id
                    || seat.original_revision_id.is_nil()
                    || seat.selected_revision_id.is_nil()
                    || !valid_entries(&seat.entries)
                    || seat.entry_ids.len() != participant.cards.len()
                    || if self.game_index.is_zero() {
                        seat.selected_revision_id != seat.original_revision_id
                            || seat.selection_decision_id.is_some()
                            || seat.selection_reply_ref.is_some()
                    } else {
                        seat.selection_decision_id.is_none() || seat.selection_reply_ref.is_none()
                    }
                {
                    return Err(NativeInitialError::ForeignSource);
                }
                let indices: BTreeMap<_, _> = seat
                    .entries
                    .iter()
                    .enumerate()
                    .map(|(n, entry)| (entry.entry_id, n))
                    .collect();
                let mut consumed: BTreeMap<Uuid, u64> = BTreeMap::new();
                seat.entry_ids
                    .iter()
                    .zip(&participant.cards)
                    .map(|(entry_id, card)| {
                        let index = *indices
                            .get(entry_id)
                            .ok_or(NativeInitialError::ForeignSource)?;
                        let entry = &seat.entries[index];
                        // Parse exact original text. No trim, case folding or opaque-ref fallback.
                        let oracle = OracleId::try_from(entry.oracle_ref.as_str().to_owned())
                            .map_err(|_| NativeInitialError::ForeignSource)?;
                        let count = consumed.entry(*entry_id).or_default();
                        *count = count.checked_add(1).ok_or(NativeInitialError::Invalid)?;
                        if oracle != card.oracle_id || Amount::from(*count) > entry.quantity {
                            return Err(NativeInitialError::ForeignSource);
                        }
                        Ok(index)
                    })
                    .collect()
            })
            .collect()
    }
    fn matches(&self, source: &NativeReservedGameSource) -> bool {
        let r = source.metadata();
        self.game_id == r.game_id
            && self.match_id == r.match_id
            && self.release_id == r.release.id
            && self.profile_id == r.profile.id
            && self.game_index == r.game_index
            && self.seats.len() == r.seats.len()
            && self.seats.iter().zip(&r.seats).all(|(a, b)| {
                a.seat_id == b.seat_id
                    && a.original_revision_id == b.original_revision_id
                    && a.original_snapshot_ref.as_str() == b.original_snapshot_ref.as_str()
                    && a.selected_revision_id == b.selected_revision_id
                    && a.selection_decision_id == b.selection_decision_id
                    && a.selection_reply_ref.as_ref().map(SourceProof::as_str)
                        == b.selection_reply_ref.as_ref().map(SourceProof::as_str)
                    && a.entries.len() == b.entries.len()
                    && a.entries.iter().zip(&b.entries).all(|(a, b)| {
                        serde_json::to_value(EntryRef(a))
                            .ok()
                            .zip(serde_json::to_value(EntryRef(b)).ok())
                            .is_some_and(|(a, b)| a == b)
                    })
            })
    }
    pub(super) fn bind(
        self,
        ids: NativeInitialIds,
        cards: BTreeMap<CardId, (usize, usize)>,
    ) -> NativeInitialProvenance {
        NativeInitialProvenance {
            ids,
            record: self,
            cards,
        }
    }
}

/// Sealed immutable physical-card mapping recovered only with its original Initial.
/// No Debug/Serialize; neither source references nor native IDs are a public view.
pub struct NativeInitialProvenance {
    ids: NativeInitialIds,
    record: SelectedRoot,
    cards: BTreeMap<CardId, (usize, usize)>,
}
impl NativeInitialProvenance {
    pub(super) fn display_context(
        &self,
        programs: &NativeProgramSource,
    ) -> Result<spellwake_engine::game::GameContext, NativeInitialError> {
        programs.context(self.record.release_id, self.record.profile_id)
    }
    /// Scope of this physical origin map, not participant authorization.
    pub fn scope(&self) -> (Uuid, Uuid) {
        (self.ids.game_id, self.ids.context_id)
    }
    /// Look up immutable card origin, never a token's copied rules or current controller.
    /// Caller must use an actual object of this checkpoint; this is not visibility.
    pub fn physical_card(&self, object: &GameObject) -> Option<NativePhysicalCardSource<'_>> {
        let (seat, entry) = self.cards.get(&object.card_id()?)?;
        Some(NativePhysicalCardSource {
            entry: self.record.seats.get(*seat)?.entries.get(*entry)?,
        })
    }
    /// Independently compare a new v2 opening against its sealed reservation source.
    /// Equality does not establish card/format qualification or provider evidence.
    pub fn matches_reserved(&self, source: &NativeReservedGameSource) -> bool {
        self.record.matches(source)
    }
}
/// Borrowed physical source identity. Printed rules and effective values are separate.
pub struct NativePhysicalCardSource<'a> {
    entry: &'a PreparationEntry,
}
impl NativePhysicalCardSource<'_> {
    /// Original selected application definition, not a copied current definition.
    pub fn definition_id(&self) -> Uuid {
        self.entry.definition_id
    }
    /// Original selected printing; None is deliberately preserved.
    pub fn printing_id(&self) -> Option<Uuid> {
        self.entry.printing.as_ref().map(|p| p.id)
    }
    /// Original selected entry identity, useful only inside trusted source joins.
    pub fn entry_id(&self) -> Uuid {
        self.entry.entry_id
    }
}
