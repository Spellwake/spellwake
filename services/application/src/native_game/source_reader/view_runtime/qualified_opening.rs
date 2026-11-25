//! Persist an opening through the original native receipts for both admission families.
use super::*;
use crate::{
    admission_ports::{AcceptReservedGame, OpeningArtifacts},
    native_game::preparation_source::{NativePreparationRepository, NativeReservedGameSource},
};
use sha2::{Digest, Sha256};
use std::error::Error;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct OpeningMaterial {
    pub(super) ready: bool,
    pub(super) context_id: Option<Uuid>,
    pub(super) first_seat_id: Option<Uuid>,
    pub(super) seed_a: Option<Uuid>,
    pub(super) seed_b: Option<Uuid>,
}

impl NativeGameRuntime {
    pub(super) fn accept_prepared_opening(
        &self,
        repository: &NativeSourceRepository,
        reservation: &NativeReservedGameSource,
        owner: &NativeHostOwner,
        context: Uuid,
        root: PreparedNativeOpening,
        context_revision: &str,
    ) -> Result<(), Box<dyn Error>> {
        let meta = reservation.metadata();
        let game = meta.game_id;
        let initial = self.store.retain(
            game,
            context,
            NativeArtifactRole::Initial,
            root.original_bytes(),
        )?;
        if &initial != root.reference() {
            return Err(invalid().into());
        }
        let checkpoint = NativeCheckpoint::initial(root)?;
        let state = checkpoint.retain(&self.store)?;
        let facts = checkpoint.prepare_results()?.retain(&self.store)?;
        let event = checkpoint.prepare_event()?.retain(&self.store)?;
        let opening = OpeningArtifacts::from_bytes(&serde_json::to_vec(&json!({
            "contextId":context,"snapshotRef":initial,"contextRevision":context_revision,
            "qualificationRef":meta.admission_qualification_ref.as_str(),"stateRef":state,
            "eventRef":event,"factsRef":facts,"eventSchemaRevision":"native-transition-v1",
            "clockObservation":checkpoint.opening_clock_observation()?
        }))?)?;
        let command = AcceptReservedGame {
            game_id: game,
            match_id: meta.match_id,
            expected_revision: meta.match_revision.clone(),
            original_accepted_basis: Some(meta.match_revision.clone()),
            owner: owner.as_str().parse()?,
            opening,
        };
        let original =
            NativeSystemOriginal::prepare_opening(&command, &checkpoint, &self.programs)?;
        repository.accept_opening(
            &self.store,
            reservation,
            &command,
            &checkpoint,
            &self.programs,
            &original,
        )?;
        Ok(())
    }
}

/// Open an exact previously admitted Constructed or Limited game.
///
/// # Errors
/// Rejects invalid worker settings, mismatched release inputs, malformed
/// opening material, and failures to record the canonical opening receipt.
pub fn open_qualified_game_from_env(game: Uuid) -> Result<bool, Box<dyn Error>> {
    super::ReleaseOpeningWorker::qualified_from_env()?.open(game)
}

pub(super) fn open_qualified(
    runtime: &NativeGameRuntime,
    owner: &NativeHostOwner,
    manifest: &AdmissionManifest,
    preparation: &NativePreparationRepository,
    repository: &NativeSourceRepository,
    reservation: &NativeReservedGameSource,
) -> Result<bool, Box<dyn Error>> {
    let meta = reservation.metadata();
    let game = meta.game_id;
    let Some(format) = manifest.reserved_format(preparation, meta)? else {
        return Ok(false);
    };
    let material: OpeningMaterial = repository.native_record(
        "read_native_opening_material",
        &json!({"p_game":game}),
        None,
    )?;
    if !material.ready {
        return Ok(false);
    }
    let context = material
        .context_id
        .filter(|id| !id.is_nil())
        .ok_or_else(invalid)?;
    let first = material.first_seat_id.ok_or_else(invalid)?;
    let seeds = [
        material.seed_a.ok_or_else(invalid)?,
        material.seed_b.ok_or_else(invalid)?,
    ];
    let mut shuffle_steps = 0usize;
    let mut largest = 0u64;
    let allow_custom =
        meta.allow_custom_cards && meta.mode == crate::decks::PlayMode::Open && !meta.competitive;
    for seat in &meta.seats {
        let (deck_size, _) = runtime.programs.inspect_selected_match_deck_with_custom(
            format,
            &seat.entries,
            &seat.custom_entries,
            allow_custom,
        )?;
        largest = largest.max(u64::from(deck_size));
        shuffle_steps += deck_size.saturating_sub(1) as usize;
    }
    let shuffles = 1 + spellwake_engine::opening::HAND_SIZE + usize::from(meta.seats.len() > 2);
    let required = (shuffle_steps * shuffles).min(8192).max(shuffle_steps);
    let entropy = opening_entropy(game, context, seeds, required, largest)?;
    let root =
        runtime
            .programs
            .prepare_reserved_opening(context, reservation, format, first, entropy)?;
    runtime.accept_prepared_opening(
        repository,
        reservation,
        owner,
        context,
        root,
        "native-constructed-opening-v1",
    )?;
    Ok(true)
}

fn opening_entropy(
    game: Uuid,
    context: Uuid,
    seeds: [Uuid; 2],
    required: usize,
    largest: u64,
) -> Result<Vec<NativeRevision>, NativeSourceError> {
    let prefix = Sha256::new()
        .chain_update(b"spellwake-native-opening-entropy-v1")
        .chain_update(game.as_bytes())
        .chain_update(context.as_bytes())
        .chain_update(seeds[0].as_bytes())
        .chain_update(seeds[1].as_bytes());
    let mut words = Vec::with_capacity(required);
    let mut guaranteed = 0;
    for counter in 0..16_384_u32 {
        let block = prefix
            .clone()
            .chain_update(counter.to_be_bytes())
            .finalize();
        for word in block.as_slice().as_chunks::<8>().0 {
            let value = u64::from_be_bytes(*word);
            words.push(NativeRevision::from(value));
            guaranteed += usize::from(value <= u64::MAX - largest);
            if guaranteed >= required {
                return Ok(words);
            }
        }
    }
    Err(invalid())
}
