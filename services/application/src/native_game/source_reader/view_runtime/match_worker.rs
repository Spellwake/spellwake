//! Private match delivery, driven by persisted reservations, accepted checkpoints and results.
use super::*;
use crate::admission_ports::{AcceptReservedGame, OpeningArtifacts};
use crate::native_game::preparation_source::NativePreparationRepository;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Material {
    ready: bool,
    context_id: Option<Uuid>,
    first_seat_id: Option<Uuid>,
    seed_a: Option<Uuid>,
    seed_b: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Page<T> {
    items: Vec<T>,
    next_after: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BetweenJob {
    id: Uuid,
    match_revision: Amount,
    profile_id: Uuid,
    release_id: Uuid,
    release_proof: String,
    selected_revision_id: Uuid,
    deadline_revision: Amount,
    request_id: Uuid,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClockJob {
    id: Uuid,
    request_id: Uuid,
    basis: PlayerBasis,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Handoff {
    id: Uuid,
    kind: String,
    game_id: Option<Uuid>,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum DeliveryStatus {
    Waiting,
    Accepted,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    status: DeliveryStatus,
}

// A word below this conservative cutoff is accepted for every supported
// shuffle bound. Counting these words prevents rejection sampling from exhausting
// the tape, without filtering or biasing any word actually delivered to the engine.
fn entropy_words(
    domain: &[u8],
    game: Uuid,
    scope: Uuid,
    seeds: (Uuid, Uuid),
    required: usize,
    largest: u64,
) -> Result<Vec<u64>, NativeSourceError> {
    let prefix = Sha256::new()
        .chain_update(domain)
        .chain_update(game.as_bytes())
        .chain_update(scope.as_bytes())
        .chain_update(seeds.0.as_bytes())
        .chain_update(seeds.1.as_bytes());
    let mut words = Vec::with_capacity(required);
    let mut guaranteed = 0;
    for counter in 0..16_384u32 {
        let block = prefix
            .clone()
            .chain_update(counter.to_be_bytes())
            .finalize();
        for word in block.as_slice().as_chunks::<8>().0 {
            let value = u64::from_be_bytes(*word);
            words.push(value);
            if value <= u64::MAX - largest {
                guaranteed += 1;
            }
            if guaranteed >= required {
                return Ok(words);
            }
        }
    }
    Err(invalid())
}

pub(super) fn refill_after_exhaustion(
    repo: &NativeSourceRepository,
    runtime: &NativeGameRuntime,
    source: &RecoveredNativeGame,
    owner: &NativeHostOwner,
) -> Result<(), NativeSourceError> {
    let checkpoint = source.checkpoint();
    let basis = checkpoint.basis();
    let id = source.source().checkpoint_ids().0;
    let (_, fence, _, _, _) = source.source().operational_basis();
    let material: Material = repo.native_record(
        "reserve_native_entropy_seed",
        &json!({"p_game":basis.game_id,
        "p_checkpoint":id,"p_owner":owner.as_str(),"p_fence":fence}),
        None,
    )?;
    if !material.ready {
        return Ok(());
    }
    let seeds = (
        material.seed_a.ok_or_else(invalid)?,
        material.seed_b.ok_or_else(invalid)?,
    );
    let words = entropy_words(
        b"spellwake-entropy-refill-v1",
        basis.game_id,
        id,
        seeds,
        8192,
        1000,
    )?;
    let seat = checkpoint.roster.first().ok_or_else(invalid)?.seat_id;
    let binding = checkpoint
        .binding(seat)
        .map_err(|e| NativeSourceError(Failure::Checkpoint(e)))?;
    let scope = NativeScope::new(checkpoint.state(), binding)
        .map_err(|e| NativeSourceError(Failure::Bridge(e)))?;
    let (bytes, expected) = scope
        .prepare_entropy(words)
        .map_err(|e| NativeSourceError(Failure::Input(e)))?
        .into_parts();
    let input = runtime
        .store
        .retain(
            basis.game_id,
            basis.context_id,
            NativeArtifactRole::Input,
            &bytes,
        )
        .map_err(|e| NativeSourceError(Failure::Artifact(e)))?;
    let err = |e| NativeSourceError(Failure::Checkpoint(e));
    let next = checkpoint
        .append(&runtime.store, input, NativeCheckpointInput::Entropy, seat)
        .map_err(err)?;
    if next.candidate() != &expected {
        return Err(invalid());
    }
    next.retain(&runtime.store).map_err(err)?;
    next.prepare_event()
        .and_then(|a| a.retain(&runtime.store))
        .map_err(err)?;
    next.prepare_results()
        .and_then(|a| a.retain(&runtime.store))
        .map_err(err)?;
    let digest = Sha256::new()
        .chain_update(b"native-entropy-request-v1")
        .chain_update(id.as_bytes())
        .finalize();
    let request = Uuid::from_slice(&digest[..16]).map_err(|_| invalid())?;
    let candidate = source.prepare_candidate(&next)?;
    let original = NativeSystemOriginal::prepare_entropy(request, source, &next)?;
    repo.commit_entropy(
        &runtime.store,
        source,
        request,
        &next,
        &candidate,
        &original,
    )?;
    Ok(())
}

/// One trusted worker over the configured immutable card package and explicit host identity.
pub struct NativeMatchWorker {
    pub(super) repo: NativeSourceRepository,
    preparation: NativePreparationRepository,
    manifest: AdmissionManifest,
    pub(super) owner: NativeHostOwner,
    runtime: NativeGameRuntime,
}

impl NativeMatchWorker {
    /// Load the pinned release and private service configuration.
    /// # Errors
    /// Returns a safe error when a required dependency is unavailable.
    pub fn from_env() -> Result<Self, Box<dyn Error>> {
        let (owner, runtime) = response_worker::owned_runtime()?;
        Ok(Self {
            repo: NativeSourceRepository::from_env()?,
            preparation: NativePreparationRepository::from_env()?,
            manifest: AdmissionManifest::from_env()?.ok_or_else(invalid)?,
            owner,
            runtime,
        })
    }

    fn open(&self, game: Uuid) -> Result<bool, Box<dyn Error>> {
        if self.repo.opening(game)?.is_some() {
            return Ok(true);
        }
        let Some(reservation) = self.preparation.reserved_game(game)? else {
            return Ok(false);
        };
        let meta = reservation.metadata();
        let Some(format) = self.manifest.reserved_format(&self.preparation, meta)? else {
            // A differently pinned worker may own this reservation.
            return Ok(false);
        };
        let material: Material = self.repo.native_record(
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
        let seeds = (
            material.seed_a.ok_or_else(invalid)?,
            material.seed_b.ok_or_else(invalid)?,
        );
        let mut shuffle_steps = 0usize;
        let mut largest = 0u64;
        for seat in &meta.seats {
            let (deck_size, _) = self
                .runtime
                .programs
                .inspect_selected_constructed_deck(format, &seat.entries)?;
            largest = largest.max(u64::from(deck_size));
            shuffle_steps += deck_size.saturating_sub(1) as usize;
        }
        // Cover setup and ordinary mulligans while keeping large tables within
        // the root envelope. A later shuffle can use the durable refill path.
        let shuffles = 1 + spellwake_engine::opening::HAND_SIZE + usize::from(meta.seats.len() > 2);
        let required = (shuffle_steps * shuffles).min(8192).max(shuffle_steps);
        let entropy = entropy_words(
            b"spellwake-native-opening-entropy-v1",
            game,
            context,
            seeds,
            required,
            largest,
        )?
        .into_iter()
        .map(NativeRevision::from)
        .collect();
        let root = self.runtime.programs.prepare_reserved_opening(
            context,
            &reservation,
            format,
            first,
            entropy,
        )?;
        let store = &self.runtime.store;
        let initial = store.retain(
            game,
            context,
            NativeArtifactRole::Initial,
            root.original_bytes(),
        )?;
        if &initial != root.reference() {
            return Err(invalid().into());
        }
        let checkpoint = NativeCheckpoint::initial(root)?;
        let state = checkpoint.retain(store)?;
        let facts = checkpoint.prepare_results()?.retain(store)?;
        let event = checkpoint.prepare_event()?.retain(store)?;
        let opening = OpeningArtifacts::from_bytes(&serde_json::to_vec(&json!({
            "contextId":context,"snapshotRef":initial,"contextRevision":"native-constructed-opening-v1",
            "qualificationRef":meta.admission_qualification_ref.as_str(),"stateRef":state,"eventRef":event,"factsRef":facts,
            "eventSchemaRevision":"native-transition-v1","clockObservation":checkpoint.opening_clock_observation()?
        }))?)?;
        let command = AcceptReservedGame {
            game_id: game,
            match_id: meta.match_id,
            expected_revision: meta.match_revision.clone(),
            original_accepted_basis: Some(meta.match_revision.clone()),
            owner: self.owner.as_str().parse()?,
            opening,
        };
        let original =
            NativeSystemOriginal::prepare_opening(&command, &checkpoint, &self.runtime.programs)?;
        self.repo.accept_opening(
            store,
            &reservation,
            &command,
            &checkpoint,
            &self.runtime.programs,
            &original,
        )?;
        Ok(true)
    }

    fn page<T: DeserializeOwned>(
        &self,
        lane: &str,
        after: Option<Uuid>,
    ) -> Result<Page<T>, NativeSourceError> {
        self.repo.native_record(
            "read_native_match_delivery_page",
            &json!({"p_owner":self.owner.as_str(),
            "p_lane":lane,"p_after":after,"p_limit":32}),
            None,
        )
    }

    fn handoff(&self, h: &Handoff) -> Result<bool, Box<dyn Error>> {
        match h.kind.as_str() {
            "ready_match" | "reserved_game" => {
                if !self.open(h.game_id.ok_or_else(invalid)?)? {
                    return Ok(false);
                }
            }
            "game_outcome" | "match_outcome" | "between_deadline" => {}
            _ => return Err(invalid().into()),
        }
        let delivery: Delivery = self.repo.native_record(
            "deliver_native_match_handoff",
            &json!({"p_handoff":h.id}),
            None,
        )?;
        Ok(matches!(delivery.status, DeliveryStatus::Accepted))
    }

    fn between(&self, job: BetweenJob) -> Result<(), Box<dyn Error>> {
        let seconds =
            self.manifest
                .between_seconds(job.release_id, &job.release_proof, job.profile_id);
        if seconds.is_none() && job.deadline_revision.is_zero() {
            return Ok(());
        }
        let intent = crate::qualified_intents::BetweenGameIntent {
            decision_id: job.id,
            expected_revision: job.match_revision,
            selected_revision_id: job.selected_revision_id,
            request_id: job.request_id,
        };
        let Some((selected, hash)) =
            self.manifest
                .between_selection(&self.preparation, &self.runtime.programs, &intent)?
        else {
            return Ok(());
        };
        let _:Delivery=self.repo.native_record("deliver_native_between_decision",&json!({
            "p_decision":job.id,"p_basis":selected.basis,"p_deadline":job.deadline_revision,"p_request":job.request_id,
            "p_seconds":seconds,"p_selected":selected.selected_revision_id,"p_hash":hash,
            "p_reply":selected.reply_ref.as_str(),"p_qualification":selected.qualification_ref.as_str()
        }),None)?;
        Ok(())
    }

    /// Walk each persisted work lane once. Independent games make progress after a job failure.
    /// # Errors
    /// Reports unreadable pages, invalid cursors, or one or more failed jobs after the sweep.
    pub fn sweep(&self) -> Result<(), Box<dyn Error>> {
        let mut failed = false;
        for lane in ["handoff", "between", "clock"] {
            let mut after = None;
            loop {
                let next = if lane == "handoff" {
                    let page = self.page::<Handoff>(lane, after)?;
                    for job in page.items {
                        if let Err(e) = self.handoff(&job) {
                            eprintln!("handoff {} failed: {e}", job.id);
                            failed = true;
                        }
                    }
                    page.next_after
                } else if lane == "between" {
                    let page = self.page::<BetweenJob>(lane, after)?;
                    for job in page.items {
                        let id = job.id;
                        if let Err(e) = self.between(job) {
                            eprintln!("between-game decision {id} failed: {e}");
                            failed = true;
                        }
                    }
                    page.next_after
                } else {
                    let page = self.page::<ClockJob>(lane, after)?;
                    for job in page.items {
                        if job.id != job.basis.game_id {
                            return Err(invalid().into());
                        }
                        if let Err(e) = response_worker::run_clock(
                            &self.repo,
                            &self.runtime,
                            &self.owner,
                            &job.basis,
                            job.request_id,
                        ) {
                            eprintln!("clock {} failed: {e}", job.id);
                            failed = true;
                        }
                    }
                    page.next_after
                };
                if next.is_none() {
                    break;
                }
                if next <= after {
                    return Err(invalid().into());
                }
                after = next;
            }
        }
        if failed {
            return Err("one or more native match deliveries failed".into());
        }
        Ok(())
    }
}
