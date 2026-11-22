//! Open and deliver previously admitted reservations through a release-bound native worker.
use super::*;
use crate::{
    account::FormatSummary,
    backend::{BackendClient, BackendConfig},
    native_game::{
        AdmissionManifest, CommanderRelease, preparation_source::NativePreparationRepository,
    },
    player_wire::PlayerBasis,
};
use sha2::{Digest, Sha256};
use std::time::Duration;

use super::qualified_opening::{OpeningMaterial, open_qualified};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpeningPage {
    items: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ClockJob {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    request_id: Uuid,
    #[serde(deserialize_with = "crate::provider_json::object")]
    basis: PlayerBasis,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockPage {
    #[serde(deserialize_with = "crate::provider_json::objects")]
    items: Vec<ClockJob>,
}

// Produce the original unfiltered entropy tape. Count words accepted by
// every permitted library bound so deterministic rejection does not exhaust it.
fn opening_entropy(
    game: Uuid,
    context: Uuid,
    seeds: [Uuid; 2],
    seats: usize,
) -> Result<Vec<NativeRevision>, NativeSourceError> {
    let prefix = Sha256::new()
        .chain_update(b"spellwake-commander-opening-entropy-v1")
        .chain_update(game.as_bytes())
        .chain_update(context.as_bytes())
        .chain_update(seeds[0].as_bytes())
        .chain_update(seeds[1].as_bytes());
    let minimum = 98 * seats * (1 + spellwake_engine::opening::HAND_SIZE + usize::from(seats > 2));
    let mut entropy = Vec::with_capacity(minimum);
    let mut accepted = 0;
    for counter in 0..16_384_u32 {
        let block = prefix
            .clone()
            .chain_update(counter.to_be_bytes())
            .finalize();
        for word in block.as_slice().as_chunks::<8>().0 {
            let value = u64::from_be_bytes(*word);
            entropy.push(NativeRevision::from(value));
            accepted += usize::from(value <= u64::MAX - 99);
            if accepted >= minimum {
                return Ok(entropy);
            }
        }
    }
    Err(invalid())
}

/// Open one already admitted Commander reservation using the configured private
/// backend. The Game ID is an identifier, not a grant of release authority.
/// Returns false for a missing reservation or a starter choice still pending.
///
/// # Errors
/// Rejects unrecognized formats, invalid reservations/releases, missing runtime
/// inputs, and failures to retain or accept the exact opening.
pub fn open_commander_game_from_env(game: Uuid) -> Result<bool, Box<dyn Error>> {
    CommanderWorker::from_env()?.open(game)
}

enum WorkerRelease {
    Commander(CommanderRelease),
    Qualified(AdmissionManifest),
}

impl WorkerRelease {
    fn id(&self) -> Uuid {
        match self {
            Self::Commander(manifest) => manifest.release_id,
            Self::Qualified(manifest) => manifest.release_id(),
        }
    }

    fn sources(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Commander(_) => (
                "read_commander_opening_page",
                "read_commander_clock_page",
                "read_commander_handoff_page",
            ),
            Self::Qualified(_) => (
                "read_private_pilot_opening_page",
                "read_private_pilot_clock_page",
                "read_private_pilot_handoff_page",
            ),
        }
    }
}

/// Release-bound native opening, clock and handoff delivery.
pub struct ReleaseOpeningWorker {
    backend: BackendClient,
    preparation: NativePreparationRepository,
    repository: NativeSourceRepository,
    runtime: NativeGameRuntime,
    owner: NativeHostOwner,
    release: WorkerRelease,
}

pub use ReleaseOpeningWorker as CommanderWorker;

impl ReleaseOpeningWorker {
    /// Load the private application connections and the approved release manifest.
    ///
    /// # Errors
    /// Rejects missing or inconsistent deployment settings and retained source inputs.
    pub fn from_env() -> Result<Self, Box<dyn Error>> {
        Self::with_release(WorkerRelease::Commander(
            CommanderRelease::from_env()?.ok_or_else(invalid)?,
        ))
    }

    /// Load the accepted Constructed and Limited pilot manifest and its private backend.
    ///
    /// # Errors
    /// Rejects invalid configuration, backend connections and unverified release inputs.
    pub fn qualified_from_env() -> Result<Self, Box<dyn Error>> {
        Self::with_release(WorkerRelease::Qualified(
            AdmissionManifest::from_env()?.ok_or_else(invalid)?,
        ))
    }

    fn with_release(release: WorkerRelease) -> Result<Self, Box<dyn Error>> {
        let base = std::env::var("SPELLWAKE_SUPABASE_URL")?;
        let secret = std::env::var("SPELLWAKE_SUPABASE_SECRET_KEY")?;
        Ok(Self {
            backend: BackendClient::new(
                BackendConfig::new(&base, &secret)?,
                "native-release-opening",
                Duration::from_secs(60),
            )?,
            preparation: NativePreparationRepository::from_env()?,
            repository: NativeSourceRepository::from_env()?,
            runtime: NativeGameRuntime::from_env(&base, &secret)?.ok_or_else(invalid)?,
            owner: NativeHostOwner::new(&std::env::var("SPELLWAKE_NATIVE_HOST_OWNER")?)?,
            release,
        })
    }

    /// Process ready openings, due opening clocks and handoffs for this release.
    ///
    /// # Errors
    /// Propagates malformed delivery pages or any independent delivery failure.
    pub fn sweep(&self) -> Result<usize, Box<dyn Error>> {
        let (openings, clocks, handoffs) = self.release.sources();
        let mut opened = 0;
        let opening = self.deliver_pages(openings, |game| {
            opened += usize::from(self.open(game)?);
            Ok(())
        });
        let clocks = self.deliver_clocks(clocks);
        let handoffs = self.deliver_pages(handoffs, |handoff| {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Delivery {
                status: String,
            }
            let delivery: Delivery = self.repository.native_record(
                "deliver_native_match_handoff",
                &json!({"p_handoff":handoff}),
                None,
            )?;
            if !matches!(delivery.status.as_str(), "accepted" | "waiting") {
                return Err(invalid().into());
            }
            Ok(())
        });
        opening?;
        clocks?;
        handoffs?;
        Ok(opened)
    }

    fn deliver_clocks(&self, name: &str) -> Result<(), Box<dyn Error>> {
        let mut after = None;
        let mut failed = false;
        loop {
            let ClockPage { items } = self.repository.native_record(
                name,
                &json!({"p_release":self.release.id(),"p_owner":self.owner.as_str(),
                    "p_after":after,"p_limit":32}),
                None,
            )?;
            if items.len() > 32
                || items.windows(2).any(|pair| pair[0].id >= pair[1].id)
                || items
                    .first()
                    .is_some_and(|job| after.is_some_and(|last| job.id <= last))
                || items.iter().any(|job| job.id != job.basis.game_id)
            {
                return Err(invalid().into());
            }
            for job in &items {
                if let Err(error) = self.expire_opening_response(job) {
                    eprintln!("native opening clock {} failed: {error}", job.id);
                    failed = true;
                }
            }
            if items.len() < 32 {
                break;
            }
            after = items.last().map(|job| job.id);
        }
        if failed {
            return Err("one or more native opening clock deliveries failed".into());
        }
        Ok(())
    }

    fn expire_opening_response(&self, job: &ClockJob) -> Result<(), Box<dyn Error>> {
        let Some(source) = self.repository.recover_owned_history(
            job.id,
            &self.runtime.store,
            &self.runtime.programs,
            self.runtime.recovery.clone(),
            &self.owner,
        )?
        else {
            return Ok(());
        };
        if source.source().context_ids().1 != self.release.id() {
            return Err(invalid().into());
        }
        let checkpoint = source.checkpoint();
        if checkpoint.basis() != &job.basis {
            return Ok(());
        }
        let Some(seat) = checkpoint.opening_response_seat()? else {
            return Ok(());
        };
        let Some(wait) = self.repository.opening_response_wait(job.id)? else {
            return Ok(());
        };
        if !wait.due_at(checkpoint) {
            return Ok(());
        }
        let proposal = wait.prepare(checkpoint)?;
        let store = &self.runtime.store;
        let input = store.retain(
            job.id,
            job.basis.context_id,
            NativeArtifactRole::Input,
            proposal.record(),
        )?;
        let next = checkpoint.append(store, input, NativeCheckpointInput::ResponseTimeout, seat)?;
        if next.candidate() != proposal.candidate() {
            return Err(invalid().into());
        }
        next.retain(store)?;
        next.prepare_event()?.retain(store)?;
        next.prepare_results()?.retain(store)?;
        let original =
            NativeSystemOriginal::prepare_response_timeout(job.request_id, &source, &next)?;
        self.repository.commit_opening_response_timeout(
            store,
            &source,
            job.request_id,
            &next,
            &original,
        )?;
        Ok(())
    }

    fn deliver_pages(
        &self,
        name: &str,
        mut deliver: impl FnMut(Uuid) -> Result<(), Box<dyn Error>>,
    ) -> Result<(), Box<dyn Error>> {
        let mut after = None;
        let mut failed = false;
        loop {
            let OpeningPage { items: page } = self.repository.native_record(
                name,
                &json!({"p_release":self.release.id(),"p_after":after,"p_limit":32}),
                None,
            )?;
            if page.len() > 32
                || page.windows(2).any(|pair| pair[0] >= pair[1])
                || page
                    .first()
                    .is_some_and(|id| id.is_nil() || after.is_some_and(|last| *id <= last))
            {
                return Err(invalid().into());
            }
            let count = page.len();
            after = page.last().copied();
            for id in page {
                if let Err(error) = deliver(id) {
                    eprintln!("native release delivery failed for {id}: {error}");
                    failed = true;
                }
            }
            if count < 32 {
                break;
            }
        }
        if failed {
            return Err("one or more native release deliveries failed".into());
        }
        Ok(())
    }

    /// Open an already admitted game for this worker's release.
    ///
    /// # Errors
    /// Rejects mismatched retained content and any failure to record the opening.
    pub fn open(&self, game: Uuid) -> Result<bool, Box<dyn Error>> {
        let Some(reservation) = self.preparation.reserved_game(game)? else {
            return Ok(false);
        };
        let manifest = match &self.release {
            WorkerRelease::Commander(manifest) => manifest,
            WorkerRelease::Qualified(manifest) => {
                return open_qualified(
                    &self.runtime,
                    &self.owner,
                    manifest,
                    &self.preparation,
                    &self.repository,
                    &reservation,
                );
            }
        };
        let meta = reservation.metadata();
        let bytes = self.backend.select_bytes_bounded(
            "format",
            &[
                ("select", "id,key,name".to_owned()),
                ("id", format!("eq.{}", meta.format_id)),
            ],
            4096,
        )?;
        let [format]: [FormatSummary; 1] = serde_json::from_slice(&bytes)?;
        if format.id != meta.format_id || format.key != "commander" {
            return Err(invalid().into());
        }
        let runtime = &self.runtime;
        if !manifest.binds(
            &runtime.programs,
            &meta.release,
            meta.format_id,
            &meta.profile,
            meta.legality_revision_id,
        ) || !meta
            .seats
            .iter()
            .all(|seat| manifest.validate_deck(&runtime.programs, &seat.entries))
        {
            return Err(invalid().into());
        }
        let material: OpeningMaterial = self.repository.native_record(
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
        let entropy = opening_entropy(game, context, seeds, meta.seats.len())?;
        let root = prepare_reserved_commander_opening(
            context,
            &runtime.programs,
            &reservation,
            &format,
            first,
            entropy,
        )?;
        runtime.accept_prepared_opening(
            &self.repository,
            &reservation,
            &self.owner,
            context,
            root,
            "native-commander-opening-v1",
        )?;
        Ok(true)
    }
}
