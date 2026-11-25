//! Bind the finite constructed setup to the match's original admission receipt.
use super::*;
use crate::native_game::preparation_source::{
    NativePreparationRepository, NativeReservedGameMetadata, NativeRoomSource, PreparationProfile,
    PreparationRelease,
};
use crate::{
    amount::Amount, catalogue_source::read_bounded, decks::PlayMode, match_history::ControllerKind,
    rooms::configuration::MatchStructure,
};
use std::{error::Error, path::Path};

const VERSION: &str = "native-constructed-ffa-candidate-v1";

#[derive(Serialize)]
struct Seat<'a> {
    index: &'a Amount,
    controller: ControllerKind,
    account: Option<Uuid>,
    sponsor: Option<Uuid>,
    original_deck: Uuid,
}

#[derive(Serialize)]
struct Selection<'a> {
    room: Uuid,
    format: Uuid,
    legality: Uuid,
    profile: Uuid,
    profile_revision: &'a str,
    profile_rules: &'a str,
    profile_evidence: &'a str,
    minimum_seats: &'a Amount,
    seat_capacity: &'a Amount,
    release: Uuid,
    release_ref: &'a str,
    release_evidence: &'a str,
    programs: &'a str,
    mode: PlayMode,
    competitive: bool,
    structure: MatchStructure,
    seats: Vec<Seat<'a>>,
}
struct Table<'a> {
    room: Uuid,
    format: Uuid,
    legality: Uuid,
    profile: &'a PreparationProfile,
    release: &'a PreparationRelease,
    mode: PlayMode,
    competitive: bool,
    structure: MatchStructure,
}
impl Table<'_> {
    fn reference(&self, key: &str, seats: Vec<Seat<'_>>) -> Result<String, NativeInitialError> {
        let p = self.profile;
        let r = self.release;
        if p.planar_pool_ref.is_some()
            || !matches!(
                key,
                "standard" | "pioneer" | "modern" | "legacy" | "vintage" | "draft" | "cube"
            )
        {
            return Err(NativeInitialError::ForeignSource);
        }
        let selection = Selection {
            room: self.room,
            format: self.format,
            legality: self.legality,
            profile: p.id,
            profile_revision: p.revision_ref.as_str(),
            profile_rules: p.rules_ref.as_str(),
            profile_evidence: p.qualification_ref.as_str(),
            minimum_seats: &p.minimum_seats,
            seat_capacity: &p.seat_capacity,
            release: r.id,
            release_ref: r.release_ref.as_str(),
            release_evidence: r.evidence_ref.as_str(),
            programs: r.program_manifest_ref.as_str(),
            mode: self.mode,
            competitive: self.competitive,
            structure: self.structure,
            seats,
        };
        let bytes =
            serde_json::to_vec(&(VERSION, key, selection)).map_err(NativeInitialError::Json)?;
        Ok(format!("{VERSION}/{key}/{}", digest(&bytes).as_str()))
    }
}
impl NativeProgramSource {
    pub(super) fn constructed_room_reference(
        &self,
        room: &NativeRoomSource,
        key: &str,
    ) -> Result<String, NativeInitialError> {
        let t = room.terms();
        let count =
            Amount::from(u64::try_from(t.slots.len()).map_err(|_| NativeInitialError::Invalid)?);
        if count < t.profile.minimum_seats {
            return Err(NativeInitialError::ForeignSource);
        }
        let mut seats = Vec::new();
        for slot in &t.slots {
            if slot.team_ref.is_some() {
                return Err(NativeInitialError::ForeignSource);
            }
            let entries = slot
                .entries
                .as_deref()
                .ok_or(NativeInitialError::ForeignSource)?;
            let custom_entries = slot.custom_entries.as_deref().unwrap_or(&[]);
            let allow_custom =
                t.room.allow_custom_cards && t.room.mode == PlayMode::Open && !t.room.competitive;
            self.verify_constructed_release_content(&t.release, entries)?;
            self.inspect_selected_match_deck_with_custom(
                key,
                entries,
                custom_entries,
                allow_custom,
            )?;
            seats.push(Seat {
                index: &slot.seat_index,
                controller: slot.controller_kind,
                account: slot.account_id,
                sponsor: slot.sponsor_id,
                original_deck: slot
                    .selected_revision_id
                    .ok_or(NativeInitialError::ForeignSource)?,
            });
        }
        Table {
            room: t.room.id,
            format: t.room.format_id,
            legality: t.legality_revision_id,
            profile: &t.profile,
            release: &t.release,
            mode: t.room.mode,
            competitive: t.room.competitive,
            structure: t.room.structure,
        }
        .reference(key, seats)
    }
}
pub(super) fn verify_reserved_format(
    source: &NativeReservedGameMetadata,
    key: &str,
) -> Result<(), NativeInitialError> {
    let seats = source
        .seats
        .iter()
        .map(|seat| Seat {
            index: &seat.seat_index,
            controller: seat.controller_kind,
            account: seat.account_id,
            sponsor: seat.sponsor_id,
            original_deck: seat.original_revision_id,
        })
        .collect();
    let expected = Table {
        room: source.room_id,
        format: source.format_id,
        legality: source.legality_revision_id,
        profile: &source.profile,
        release: &source.release,
        mode: source.mode,
        competitive: source.competitive,
        structure: source.structure,
    }
    .reference(key, seats)?;
    if expected != source.admission_qualification_ref.as_str() {
        return Err(NativeInitialError::ForeignSource);
    }
    Ok(())
}

/// Build the local candidate's setup reference from a service-owned room and format read.
/// The public release decision remains separate from this candidate reference.
/// # Errors
/// Rejects unavailable source, mixed teams, invalid deck contents and mismatched runtime inputs.
pub fn inspect_constructed_room(
    metadata: &Path,
    snapshot: &Path,
    vocabulary: &Path,
    room: Uuid,
    basis: &Amount,
    release: Uuid,
    legality: Uuid,
) -> Result<String, Box<dyn Error>> {
    let repository = NativePreparationRepository::from_env()?;
    let room = repository
        .room(room, basis, release, legality)?
        .ok_or(NativeInitialError::ForeignSource)?;
    let key = repository.constructed_format_key(room.terms().room.format_id)?;
    let programs = NativeProgramSource::from_bytes(
        NativeProgramPool::ResolutionSelectionNaturalMana,
        &read_bounded(metadata, 1_048_576)?,
        read_bounded(snapshot, MAX_NATIVE_ARTIFACT_BYTES)?,
        &read_bounded(vocabulary, 1_048_576)?,
    )?;
    Ok(programs.constructed_room_reference(&room, &key)?)
}
