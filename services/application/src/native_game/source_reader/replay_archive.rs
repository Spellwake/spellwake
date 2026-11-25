//! Portable retained participant replay: a private content-addressed archive,
//! canonical once-only SQL publication and a reader that needs no original
//! native runtime, program source or recovery.
//!
//! Archive v2 (`spellwake-replay-v2`), every artifact private Replay role:
//! - header (manifest `artifact_ref`): scope, release, schema, index ref;
//! - index (manifest `visibility_index_ref`): frontier, opening/initial
//!   checkpoint, original opening lineage (native epoch, runtime, program
//!   source, qualification proof, initial artifacts), original vocabulary
//!   ref/id, the conclusion kind, and per seat × disclosure the ordered page
//!   ranges;
//! - pages: at most 64 consecutive cuts, each the SQL-winning View identity,
//!   accepted event/checkpoint lineage, native terminal and the projected
//!   snapshot with its independently captured validation metadata;
//! - the exact original vocabulary input bytes.
//! - the original canonical program identity descriptor binding those bytes.
//!
//! Completed native games are archived under the same completion authority as
//! full reveal: seat-filtered pages for every human seat and full pages for
//! every seat. An accepted operational no-contest archives only seat-filtered
//! prefix pages for human seats. Games in progress are never archived.
use super::cuts::{CutLineage, CutSource, HistoryCut, projection_error};
use super::historical::ArchivePerspective;
use super::*;
use crate::{
    native_game::projection::{ArchivedProjection, RestoredProjection},
    player_wire::{PlayerProjectionFacts, PlayerSnapshot, PlayerTerminal},
    recovery_ports::PublishReplay,
    source_protocol::SourceProof,
};
use serde::Serialize;
use spellwake_contracts::creature_types::{CreatureTypeVocabulary, CreatureVocabularyId};
use std::{str::FromStr, sync::Mutex, time::Instant};
mod integrity;
#[cfg(test)]
pub(super) mod projection_fixture;

/// Codec label stored in the manifest and returned to replay clients.
pub(super) const REPLAY_SCHEMA: &str = "spellwake-replay-v2";
const MAX_CUTS_PER_PAGE: usize = 64;
/// Bounded root fanout per perspective; larger games refuse honestly.
const MAX_PAGES: usize = 4_096;
const MAX_MANIFEST_BYTES: u64 = 16_384;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedCut {
    sequence: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    view_receipt_id: Uuid,
    view_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    event_id: Uuid,
    event_ref: NativeArtifactRef,
    state_ref: NativeArtifactRef,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    checkpoint_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    creator_receipt_id: Uuid,
    #[serde(deserialize_with = "Option::deserialize")]
    terminal: Option<PlayerTerminal>,
    #[serde(deserialize_with = "crate::provider_json::object")]
    projection: ArchivedProjection,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedPage {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    perspective_seat_id: Uuid,
    disclosure: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    cuts: Vec<EncodedCut>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedPageRef {
    first: Amount,
    last: Amount,
    page_ref: NativeArtifactRef,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedPerspective {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    seat_id: Uuid,
    disclosure: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    pages: Vec<EncodedPageRef>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedIndex {
    version: String,
    #[serde(deserialize_with = "crate::provider_json::object")]
    lineage: EncodedLineage,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    opening_receipt_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    initial_checkpoint_id: Uuid,
    through_event_sequence: Amount,
    through_revision: Amount,
    vocabulary_ref: NativeArtifactRef,
    vocabulary_id: CreatureVocabularyId,
    program_identity_ref: NativeArtifactRef,
    /// `native-conclusion` (terminal at the last cut, seat-filtered and full
    /// pages) or `operational-incident` (no terminal, seat-filtered only).
    conclusion: String,
    #[serde(deserialize_with = "crate::provider_json::objects")]
    perspectives: Vec<EncodedPerspective>,
}
/// Original opening identity the archive was produced under. The reader
/// compares it with the game's accepted opening record, not the current runtime.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedLineage {
    native_epoch: NativeRevision,
    runtime_id: String,
    program_source_id: String,
    qualification_ref: String,
    initial_ref: NativeArtifactRef,
    initial_state_ref: NativeArtifactRef,
}
impl EncodedLineage {
    fn of(opening: &NativeOpeningSource) -> Self {
        Self {
            native_epoch: opening.native_epoch(),
            runtime_id: opening.runtime_id().to_owned(),
            program_source_id: opening.program_source_id().to_owned(),
            qualification_ref: opening.artifacts().qualification_ref().to_owned(),
            initial_ref: opening.artifacts().initial().clone(),
            initial_state_ref: opening.artifacts().state().clone(),
        }
    }
}
const NATIVE_CONCLUSION: &str = "native-conclusion";
const OPERATIONAL_INCIDENT: &str = "operational-incident";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EncodedHeader {
    version: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    release_id: Uuid,
    schema_revision: String,
    index_ref: NativeArtifactRef,
}
/// Manifest row read by its exact id/game/context. Numeric columns are
/// selected as text so frontiers decode losslessly.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct ManifestRow {
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    game_id: Uuid,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    context_id: Uuid,
    artifact_ref: NativeArtifactRef,
    visibility_index_ref: NativeArtifactRef,
    schema_revision: String,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    initial_checkpoint_id: Uuid,
    through_event_sequence: Amount,
    through_accepted_revision: Amount,
    #[serde(deserialize_with = "crate::identifiers::uuid")]
    source_receipt_id: Uuid,
    qualification_ref: String,
}

fn label(disclosure: NativeDisclosure) -> &'static str {
    match disclosure {
        NativeDisclosure::LiveParticipant => "live",
        NativeDisclosure::CompletedParticipant => "completed",
    }
}
// Test-only stage observations keep corruption controls causal without changing
// production error types or exposing private diagnostics at the public boundary.
#[cfg(test)]
thread_local! { static LAST_FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
pub(super) fn take_test_fault() -> Option<&'static str> {
    LAST_FAULT.with(std::cell::Cell::take)
}
fn note_fault(stage: &'static str) {
    #[cfg(test)]
    {
        LAST_FAULT.with(|last| last.set(Some(stage)));
        eprintln!("REPLAY_FAULT {stage}");
    }
    let _ = stage;
}
fn fault(stage: &'static str) -> NativeSourceError {
    note_fault(stage);
    invalid()
}
fn json_error(error: serde_json::Error) -> NativeSourceError {
    NativeSourceError(Failure::Backend(BackendError::caused("json", error)))
}
fn artifact_error(error: NativeArtifactError) -> NativeSourceError {
    NativeSourceError(Failure::Artifact(error))
}
fn qualification(header: &NativeArtifactRef) -> String {
    format!("{REPLAY_SCHEMA}:{}", header.as_str())
}
fn proof(text: &str) -> Result<SourceProof, NativeSourceError> {
    SourceProof::from_str(text).map_err(|_| fault("publication proof"))
}
fn charge(
    budget: &mut NativeRecoveryBudget,
    reference: &NativeArtifactRef,
) -> Result<(), NativeSourceError> {
    budget
        .charge_work(1, reference.size())
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))
}
/// Reserve one private Replay artifact and its payload bytes before Store IO.
/// Failed storage does not refund attempt work or roll back earlier artifacts.
fn retain(
    store: &NativeArtifactStore,
    budget: &mut NativeRecoveryBudget,
    (game, context): (Uuid, Uuid),
    bytes: &[u8],
) -> Result<NativeArtifactRef, NativeSourceError> {
    let reference = NativeArtifactRef::for_bytes(game, context, NativeArtifactRole::Replay, bytes)
        .map_err(artifact_error)?;
    charge(budget, &reference)?;
    let retained = store
        .retain(game, context, NativeArtifactRole::Replay, bytes)
        .map_err(artifact_error)?;
    if retained != reference {
        return Err(fault("retained reference"));
    }
    Ok(reference)
}
/// One aggregate build budget: the per-walk template times the planned walks.
fn scaled(
    template: &NativeRecoveryBudget,
    walks: usize,
) -> Result<NativeRecoveryBudget, NativeSourceError> {
    template
        .scaled(walks)
        .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))
}

#[cfg(test)]
std::thread_local! {
    static TEST_PUBLICATION_OBSERVATIONS: std::cell::RefCell<[Option<serde_json::Value>;2]> = const {std::cell::RefCell::new([None,None])};
}
/// Numeric diagnostic only, from the synchronous publisher on this calling thread.
/// It cannot construct native access, budget authority or a guarded response.
#[cfg(test)]
pub(super) fn take_test_publication_observation(complete: bool) -> Option<serde_json::Value> {
    TEST_PUBLICATION_OBSERVATIONS.with(|slots| slots.borrow_mut()[usize::from(complete)].take())
}
#[cfg(test)]
fn observe_publication_budget(value: serde_json::Value, complete: bool) {
    TEST_PUBLICATION_OBSERVATIONS
        .with(|slots| slots.borrow_mut()[usize::from(complete)] = Some(value.clone()));
    let marker = if complete {
        "NATIVE_REPLAY_PUBLICATION_BUDGET"
    } else {
        "NATIVE_REPLAY_PUBLICATION_SHARED_BEGIN"
    };
    println!("{marker} {value}");
}
#[cfg(test)]
fn budget_amount((records, bytes): (usize, u64)) -> serde_json::Value {
    json!({"records":records,"bytes":bytes})
}
#[cfg(test)]
fn budget_spent(initial: (usize, u64), remaining: (usize, u64)) -> serde_json::Value {
    budget_amount((
        initial
            .0
            .checked_sub(remaining.0)
            .expect("test budget only debits records"),
        initial
            .1
            .checked_sub(remaining.1)
            .expect("test budget only debits bytes"),
    ))
}

/// One archived cut restored from hash-verified published bytes and validated
/// against the archive's original vocabulary. Not native output.
pub(in crate::native_game) struct ArchivedCut {
    view_receipt_id: Uuid,
    event_id: Uuid,
    event_ref: NativeArtifactRef,
    state_ref: NativeArtifactRef,
    checkpoint_id: Uuid,
    creator_receipt_id: Uuid,
    terminal: Option<PlayerTerminal>,
    projection: RestoredProjection,
}
impl HistoryCut for ArchivedCut {
    fn snapshot(&self) -> &PlayerSnapshot {
        self.projection.snapshot()
    }
    fn cut_facts<'a>(
        &'a self,
        vocabulary: &'a CreatureTypeVocabulary,
    ) -> Result<PlayerProjectionFacts<'a>, NativeSourceError> {
        self.projection.facts(vocabulary).map_err(projection_error)
    }
    fn cut_terminal(&self) -> Option<&PlayerTerminal> {
        self.terminal.as_ref()
    }
    fn view_receipt(&self) -> Uuid {
        self.view_receipt_id
    }
    fn lineage(&self) -> CutLineage<'_> {
        CutLineage {
            event_id: self.event_id,
            event_ref: &self.event_ref,
            state_ref: &self.state_ref,
            checkpoint_id: self.checkpoint_id,
            creator_receipt_id: self.creator_receipt_id,
        }
    }
}
/// Exact game/context and the manifest selected by current participant access.
#[derive(Clone, Copy)]
pub(in crate::native_game) struct ReplayTarget {
    pub(in crate::native_game) game: Uuid,
    pub(in crate::native_game) context: Uuid,
    pub(in crate::native_game) manifest: Uuid,
}
/// One verified perspective/disclosure of a published archive.
pub(in crate::native_game) struct PortableReplay {
    manifest_id: Uuid,
    release_id: Uuid,
    opening_receipt_id: Uuid,
    disclosure: NativeDisclosure,
    vocabulary: CreatureTypeVocabulary,
    cuts: Vec<ArchivedCut>,
}
impl PortableReplay {
    pub(super) fn cut_source(&self) -> CutSource<'_, ArchivedCut> {
        CutSource {
            cuts: &self.cuts,
            disclosure: self.disclosure,
            opening_receipt_id: self.opening_receipt_id,
            vocabulary: &self.vocabulary,
        }
    }
    /// Original accepted release of the archived game.
    pub(in crate::native_game) fn release_id(&self) -> Uuid {
        self.release_id
    }
    /// Canonical manifest this archive was read through.
    pub(in crate::native_game) fn manifest_id(&self) -> Uuid {
        self.manifest_id
    }
}

impl NativeSourceRepository {
    fn retain_perspective(
        store: &NativeArtifactStore,
        history: &ArchivePerspective,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<EncodedPerspective, NativeSourceError> {
        let disclosure = label(history.disclosure);
        let seat = history.seat;
        let tip = history
            .observations
            .last()
            .ok_or_else(invalid)?
            .projection()
            .snapshot();
        let (game, context) = (tip.basis.game_id, tip.basis.context_id);
        let mut pages = Vec::new();
        for chunk in history.observations.chunks(MAX_CUTS_PER_PAGE) {
            let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else {
                return Err(fault("empty page chunk"));
            };
            let page = EncodedPage {
                version: REPLAY_SCHEMA.to_owned(),
                game_id: game,
                context_id: context,
                perspective_seat_id: seat,
                disclosure: disclosure.to_owned(),
                cuts: chunk
                    .iter()
                    .map(|cut| EncodedCut {
                        sequence: cut.projection.snapshot().through_event_sequence.clone(),
                        view_receipt_id: cut.receipt_id,
                        view_ref: cut.view_ref.clone(),
                        event_id: cut.event_id,
                        event_ref: cut.event_ref.clone(),
                        state_ref: cut.state_ref.clone(),
                        checkpoint_id: cut.checkpoint_id,
                        creator_receipt_id: cut.creator_receipt_id,
                        terminal: cut.terminal.clone(),
                        projection: cut.projection.archive(),
                    })
                    .collect(),
            };
            let bytes = serde_json::to_vec(&page).map_err(json_error)?;
            let page_ref = retain(store, budget, (game, context), &bytes)?;
            pages.push(EncodedPageRef {
                first: first.projection.snapshot().through_event_sequence.clone(),
                last: last.projection.snapshot().through_event_sequence.clone(),
                page_ref,
            });
        }
        if pages.len() > MAX_PAGES {
            return Err(fault("page fanout"));
        }
        Ok(EncodedPerspective {
            seat_id: seat,
            disclosure: disclosure.to_owned(),
            pages,
        })
    }
    /// Build, retain and canonically publish the archive of a completed game or
    /// accepted operational no-contest, then return the SQL-winning manifest id.
    /// One native walk proves completion and another projects every perspective.
    /// The viewer retains its own allowance and the other views and archive
    /// bytes share the scaled allowance.
    /// Deterministic encoding over SQL-winning Views lets concurrent builders
    /// converge on one publication; SQL rejects any different replacement.
    ///
    /// The caller must still renew participant access before delivery.
    /// # Errors
    /// Rejects games with neither actual native and SQL completion nor an
    /// accepted operational no-contest, a full walk of an incident,
    /// lineage drift, vocabulary mismatch, oversize archives, retention/budget
    /// failures and any publication fault. None means the game disappeared.
    pub(in crate::native_game) fn publish_portable_replay(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        template: &NativeRecoveryBudget,
    ) -> Result<Option<Uuid>, NativeSourceError> {
        let started = Instant::now();
        let completed = access.completed_disclosure().is_some();
        if !completed && !access.operational_no_contest() {
            return Err(fault("not archivable"));
        }
        let Some(authority) =
            self.archive_completion_authority(store, programs, access, template.clone())?
        else {
            return Ok(None);
        };
        let proof_elapsed = started.elapsed();
        let roster = &authority.roster;
        let humans = &authority.humans;
        let viewer = access.viewer_seat_id();
        if roster.is_empty()
            || roster.len() > 6
            || humans.is_empty()
            || !humans.contains(&viewer)
            || humans.iter().any(|seat| !roster.contains(seat))
        {
            return Err(fault("roster"));
        }
        let planned = humans.len() + if completed { roster.len() } else { 0 };
        let mut first_budget = template.clone();
        let mut budget = scaled(template, planned.saturating_sub(1).max(1))?;
        #[cfg(test)]
        let observed_shared_initial = budget.remaining();
        authority.charge_recovery(&mut first_budget, 1)?;
        authority.charge_recovery(&mut budget, planned.saturating_sub(1))?;
        let first_budget = Mutex::new(first_budget);
        let shared_budget = Mutex::new(budget);
        let Some(history) = self.collect_archive_history(
            store,
            programs,
            &authority,
            template.clone(),
            &first_budget,
            &shared_budget,
        )?
        else {
            return Ok(None);
        };
        #[cfg(test)]
        let observed_first_remaining = first_budget
            .into_inner()
            .map_err(|_| invalid())?
            .remaining();
        let mut budget = shared_budget.into_inner().map_err(|_| invalid())?;
        #[cfg(test)]
        let observed_shared_views_remaining = budget.remaining();
        let projections_elapsed = started.elapsed().saturating_sub(proof_elapsed);
        let first = history
            .perspectives
            .iter()
            .find(|part| {
                part.seat == viewer
                    && (part.disclosure == NativeDisclosure::CompletedParticipant) == completed
            })
            .ok_or_else(invalid)?;
        let tip = first.observations.last().ok_or_else(invalid)?;
        let snapshot = tip.projection.snapshot();
        let basis = snapshot.basis.clone();
        let through = snapshot.through_event_sequence.clone();
        if tip.terminal.is_some() != completed
            || !snapshot
                .players
                .iter()
                .map(|player| player.seat_id)
                .eq(roster.iter().copied())
        {
            return Err(fault("roster"));
        }
        let cuts = first.observations.len();
        let source = history.recovered.source();
        let release_id = source.context_ids().1;
        let opening_source = history.recovered.opening();
        let opening = opening_source.receipt_ids();
        let opening_lineage = EncodedLineage::of(opening_source);
        let vocabulary_id = history
            .recovered
            .checkpoint()
            .state()
            .execution()
            .creature_vocabulary()
            .id();
        let (game, context) = (basis.game_id, basis.context_id);
        let program_identity_ref = retain(
            store,
            &mut budget,
            (game, context),
            programs.program_identity_bytes(),
        )?;
        let input = programs.vocabulary_input();
        let original = crate::native_game::initial_source::decode_vocabulary_input(input)
            .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        if original.id() != vocabulary_id {
            return Err(fault("vocabulary identity"));
        }
        let vocabulary_ref = retain(store, &mut budget, (game, context), input)?;
        let mut perspectives = Vec::with_capacity(planned);
        for part in &history.perspectives {
            if part.observations.len() != cuts
                || !part.observations.last().is_some_and(|cut| {
                    cut.projection.snapshot().basis == basis && cut.terminal.is_some() == completed
                })
            {
                return Err(fault("archive lineage"));
            }
            perspectives.push(Self::retain_perspective(store, part, &mut budget)?);
        }
        let revision = Amount::from(basis.revision.get());
        let index = EncodedIndex {
            version: REPLAY_SCHEMA.to_owned(),
            lineage: opening_lineage,
            game_id: game,
            context_id: context,
            release_id,
            opening_receipt_id: opening.0,
            initial_checkpoint_id: opening.2,
            through_event_sequence: through.clone(),
            through_revision: revision.clone(),
            vocabulary_ref,
            vocabulary_id,
            program_identity_ref,
            conclusion: if completed {
                NATIVE_CONCLUSION
            } else {
                OPERATIONAL_INCIDENT
            }
            .to_owned(),
            perspectives,
        };
        let index_ref = retain(
            store,
            &mut budget,
            (game, context),
            &serde_json::to_vec(&index).map_err(json_error)?,
        )?;
        let header = EncodedHeader {
            version: REPLAY_SCHEMA.to_owned(),
            game_id: game,
            context_id: context,
            release_id,
            schema_revision: REPLAY_SCHEMA.to_owned(),
            index_ref: index_ref.clone(),
        };
        let header_ref = retain(
            store,
            &mut budget,
            (game, context),
            &serde_json::to_vec(&header).map_err(json_error)?,
        )?;
        let publication = PublishReplay {
            game_id: game,
            revision: revision.clone(),
            initial_checkpoint_id: opening.2,
            through_event_sequence: through.clone(),
            artifact_ref: proof(header_ref.as_str())?,
            visibility_index_ref: proof(index_ref.as_str())?,
            schema_revision: proof(REPLAY_SCHEMA)?,
            qualification_ref: proof(&qualification(&header_ref))?,
        };
        let bytes = self
            .backend
            .rpc_bytes_bounded(
                "publish_game_replay",
                &json!({"p_game":game,"p_revision":revision,"p_checkpoint":opening.2,
                    "p_through":through,"p_artifact":header_ref.as_str(),
                    "p_visibility":index_ref.as_str(),"p_schema":REPLAY_SCHEMA,
                    "p_qualification":qualification(&header_ref)}),
                65_536,
            )
            .map_err(|error| {
                #[cfg(test)]
                if matches!(&error, BackendError::RequestCause { stage: "body", .. }) {
                    println!("REPLAY_PUBLICATION_BODY_TRANSPORT_FAILURE");
                }
                NativeSourceError(Failure::Backend(error))
            })?;
        let ack = publication
            .decode_response(&bytes)
            .map_err(|_| fault("publication ack"))?;
        #[cfg(test)]
        {
            // Invocation accounting includes duplicate refs: each actual retain
            // reserved its payload once. This is not total Store/network IO.
            let retained_sizes: Vec<_> =
                [
                    index.program_identity_ref.size(),
                    index.vocabulary_ref.size(),
                ]
                .into_iter()
                .chain(index.perspectives.iter().flat_map(|perspective| {
                    perspective.pages.iter().map(|page| page.page_ref.size())
                }))
                .chain([index_ref.size(), header_ref.size()])
                .collect();
            let retained_bytes = retained_sizes
                .iter()
                .try_fold(0_u64, |sum, size| sum.checked_add(*size))
                .expect("bounded test publication payload sum");
            observe_publication_budget(
                json!({"completed":completed,"humanSeats":humans.len(),"rosterSeats":roster.len(),
                "plannedPerspectives":planned,"nativeWalks":2,"firstTemplate":budget_amount(template.remaining()),
                "firstSpend":budget_spent(template.remaining(),observed_first_remaining),"firstRemaining":budget_amount(observed_first_remaining),
                "sharedCapacity":budget_amount(observed_shared_initial),"sharedSpend":budget_spent(observed_shared_initial,budget.remaining()),
                "sharedProjectionSpend":budget_spent(observed_shared_initial,observed_shared_views_remaining),
                "sharedRemaining":budget_amount(budget.remaining()),
                "retainedPayloadOperations":retained_sizes.len(),"retainedPayloadBytes":retained_bytes,
                "pageCounts":index.perspectives.iter().map(|perspective|perspective.pages.len()).collect::<Vec<_>>(),
                "cutsPerPerspective":cuts,"publicationAckResponseCap":65536,
                "accounting":"recovery work and retained payload; excludes separate publication ack and total network IO"}),
                true,
            );
        }
        eprintln!(
            "replay_archive outcome=published native_walks=2 perspectives={planned} cuts={cuts} proof_ms={} projection_ms={} total_ms={}",
            proof_elapsed.as_millis(),
            projections_elapsed.as_millis(),
            started.elapsed().as_millis(),
        );
        Ok(Some(ack.resource_id))
    }

    fn read_manifest(
        &self,
        game: Uuid,
        context: Uuid,
        manifest: Uuid,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<ManifestRow, NativeSourceError> {
        if game.is_nil() || context.is_nil() || manifest.is_nil() {
            return Err(fault("nil manifest scope"));
        }
        let rows: Vec<ManifestRow> = self.native_rows(
            "replay_manifest",
            &[
                (
                    "select",
                    "id,game_id,context_id,artifact_ref,visibility_index_ref,schema_revision,\
                         initial_checkpoint_id,through_event_sequence::text,\
                         through_accepted_revision::text,source_receipt_id,qualification_ref"
                        .to_owned(),
                ),
                ("id", format!("eq.{manifest}")),
                ("game_id", format!("eq.{game}")),
                ("context_id", format!("eq.{context}")),
                ("limit", "2".to_owned()),
            ],
            MAX_MANIFEST_BYTES,
            budget,
        )?;
        let [row] = <[ManifestRow; 1]>::try_from(rows).map_err(|_| fault("manifest rows"))?;
        if row.id != manifest
            || row.game_id != game
            || row.context_id != context
            || row.schema_revision != REPLAY_SCHEMA
            || row.source_receipt_id.is_nil()
            || row.qualification_ref != qualification(&row.artifact_ref)
        {
            return Err(fault("manifest fields"));
        }
        Ok(row)
    }
    /// Read and verify one perspective/disclosure of a published archive. Takes
    /// no program source or native runtime: archived observations are never
    /// reinterpreted by the current engine. Participant authority is the
    /// caller's; this only verifies lineage, bounds and projection metadata.
    /// # Errors
    /// None means the perspective is not a retained seat of this archive.
    /// Rejects missing/foreign/unknown-codec manifests, hash/scope/role faults,
    /// any field drift between manifest, header, index and pages, gaps or
    /// overlaps, a different vocabulary, invalid snapshots and exhausted budget.
    pub(in crate::native_game) fn read_portable_replay(
        &self,
        store: &NativeArtifactStore,
        target: ReplayTarget,
        perspective: Uuid,
        disclosure: NativeDisclosure,
        until: Option<&Amount>,
        budget: &mut NativeRecoveryBudget,
    ) -> Result<Option<PortableReplay>, NativeSourceError> {
        let ReplayTarget {
            game,
            context,
            manifest,
        } = target;
        let row = self.read_manifest(game, context, manifest, budget)?;
        let read = |reference: &NativeArtifactRef, budget: &mut NativeRecoveryBudget| {
            charge(budget, reference)?;
            store
                .read(reference, game, context, NativeArtifactRole::Replay)
                .map_err(artifact_error)
        };
        let header: EncodedHeader =
            crate::decode_wire_object(read(&row.artifact_ref, budget)?.bytes())
                .map_err(json_error)?;
        verify_header(&row, &header)?;
        let index: EncodedIndex =
            crate::decode_wire_object(read(&header.index_ref, budget)?.bytes())
                .map_err(json_error)?;
        // Bind the archive to the game's accepted original opening record.
        let opening = self
            .opening_bounded(game, Some(budget))?
            .ok_or_else(|| fault("original opening unavailable"))?;
        let native_conclusion = verify_index(
            &row,
            &header,
            &index,
            &OpeningFacts::of(&opening, game, context)?,
        )?;
        let source = self.read_bounded(game, Some(budget))?.ok_or_else(invalid)?;
        if source.context_ids() != (context, index.release_id, source.context_ids().2)
            || source.record.initial_ref != *opening.artifacts().initial()
            || source.last_event_sequence() != &index.through_event_sequence
            || source.accepted_revision() != &index.through_revision
            || source.phases().0
                != if native_conclusion {
                    GamePhase::Completed
                } else {
                    GamePhase::NoContest
                }
        {
            return Err(fault("archive original source"));
        }
        if index.program_identity_ref.size() > 16_384 || index.vocabulary_ref.size() > 1_048_576 {
            return Err(fault("original input size"));
        }
        let vocabulary_bytes = read(&index.vocabulary_ref, budget)?;
        let descriptor = read(&index.program_identity_ref, budget)?;
        let program_hash = crate::price_source::EvidenceHash::try_from(format!(
            "sha256:{}",
            opening.program_source_id()
        ))
        .map_err(|_| fault("original program identity"))?;
        let program = crate::native_game::initial_source::verify_program_identity(
            descriptor.bytes(),
            vocabulary_bytes.bytes(),
            opening.runtime_id(),
            &program_hash,
        )
        .map_err(|error| {
            note_fault("original program identity");
            NativeSourceError(Failure::Initial(error))
        })?;
        let vocabulary =
            crate::native_game::initial_source::decode_vocabulary_input(vocabulary_bytes.bytes())
                .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        if vocabulary.id() != index.vocabulary_id {
            return Err(fault("vocabulary"));
        }
        let wanted = label(disclosure);
        let full = disclosure == NativeDisclosure::CompletedParticipant;
        let Some(entry) = select_perspective(&index, perspective, disclosure, native_conclusion)?
        else {
            // Not a retained seat of this game.
            return Ok(None);
        };
        let roster = source
            .controllers()
            .enumerate()
            .map(|(index, seat)| Ok((seat.0, u64::try_from(index).map_err(|_| invalid())?)))
            .collect::<Result<Vec<_>, NativeSourceError>>()?;
        let originals = integrity::Originals {
            source: &source,
            opening: &opening,
            program: &program,
            program_hash: &program_hash,
            vocabulary: &vocabulary,
            roster: &roster,
            perspective,
            disclosure,
        };
        let terminal = self.portable_terminal_source(&source, &index, native_conclusion, budget)?;
        let mut cuts = Vec::new();
        let mut expected = Amount::from(0);
        let mut previous_revision = None;
        for page_ref in &entry.pages {
            // Pages after the requested coordinate are never needed.
            if until.is_some_and(|until| page_ref.first > *until) {
                break;
            }
            let page: EncodedPage =
                crate::decode_wire_object(read(&page_ref.page_ref, budget)?.bytes())
                    .map_err(json_error)?;
            if page.version != REPLAY_SCHEMA
                || page.game_id != game
                || page.context_id != context
                || page.perspective_seat_id != perspective
                || page.disclosure != wanted
                || page.cuts.is_empty()
                || page.cuts.len() > MAX_CUTS_PER_PAGE
                || page_ref.first != expected
                || page.cuts.first().map(|cut| &cut.sequence) != Some(&page_ref.first)
                || page.cuts.last().map(|cut| &cut.sequence) != Some(&page_ref.last)
            {
                return Err(fault("page"));
            }
            for cut in page.cuts {
                let terminal_allowed =
                    native_conclusion && cut.sequence == index.through_event_sequence;
                if cut.sequence != expected
                    || cut.terminal.is_some() != terminal_allowed
                    || [&cut.view_ref, &cut.event_ref, &cut.state_ref]
                        .iter()
                        .any(|reference| {
                            reference.game_id() != game || reference.context_id() != context
                        })
                    || [
                        cut.view_ref.role(),
                        cut.event_ref.role(),
                        cut.state_ref.role(),
                    ] != [
                        NativeArtifactRole::View,
                        NativeArtifactRole::Event,
                        NativeArtifactRole::State,
                    ]
                {
                    return Err(fault("cut lineage"));
                }
                originals.verify_view(self, store, &cut, budget)?;
                let projection = cut.projection.restore();
                let snapshot = projection.snapshot();
                let facts = projection.facts(&vocabulary).map_err(projection_error)?;
                snapshot
                    .validate(&facts)
                    .map_err(|_| fault("snapshot validation"))?;
                if snapshot.basis.game_id != game
                    || snapshot.basis.context_id != context
                    || snapshot.viewer_seat_id != perspective
                    || snapshot.through_event_sequence != cut.sequence
                    || projection.full_disclosure() != full
                    || previous_revision.is_some_and(|r| snapshot.basis.revision < r)
                {
                    return Err(fault("snapshot scope"));
                }
                previous_revision = Some(snapshot.basis.revision);
                expected = cut
                    .sequence
                    .checked_add(&Amount::from(1))
                    .map_err(|_| fault("sequence overflow"))?;
                cuts.push(ArchivedCut {
                    view_receipt_id: cut.view_receipt_id,
                    event_id: cut.event_id,
                    event_ref: cut.event_ref,
                    state_ref: cut.state_ref,
                    checkpoint_id: cut.checkpoint_id,
                    creator_receipt_id: cut.creator_receipt_id,
                    terminal: cut.terminal,
                    projection,
                });
            }
        }
        let tip = cuts.last().ok_or_else(invalid)?.snapshot();
        let complete = tip.through_event_sequence == index.through_event_sequence;
        if complete && Amount::from(tip.basis.revision.get()) != index.through_revision
            || !complete && until.is_none_or(|until| tip.through_event_sequence < *until)
        {
            return Err(fault("frontier"));
        }
        integrity::verify_terminal(&cuts, &index, terminal.as_ref())?;
        self.verify_portable_cut_sources(
            &CutSource {
                cuts: &cuts,
                disclosure,
                opening_receipt_id: index.opening_receipt_id,
                vocabulary: &vocabulary,
            },
            budget,
        )?;
        Ok(Some(PortableReplay {
            manifest_id: row.id,
            release_id: index.release_id,
            opening_receipt_id: index.opening_receipt_id,
            disclosure,
            vocabulary,
            cuts,
        }))
    }
}

/// Accepted original opening facts an archive must bind.
struct OpeningFacts {
    game: Uuid,
    context: Uuid,
    lineage: EncodedLineage,
    opening_receipt_id: Uuid,
    initial_checkpoint_id: Uuid,
}
impl OpeningFacts {
    fn of(
        opening: &NativeOpeningSource,
        game: Uuid,
        context: Uuid,
    ) -> Result<Self, NativeSourceError> {
        let (_, opening_game, opening_context) = opening.ids();
        if opening_game != game || opening_context != context {
            return Err(fault("lineage"));
        }
        let receipts = opening.receipt_ids();
        Ok(Self {
            game,
            context,
            lineage: EncodedLineage::of(opening),
            opening_receipt_id: receipts.0,
            initial_checkpoint_id: receipts.2,
        })
    }
}
/// Header bound to its exact manifest row, scope and codec.
fn verify_header(row: &ManifestRow, header: &EncodedHeader) -> Result<(), NativeSourceError> {
    if header.version != REPLAY_SCHEMA
        || header.schema_revision != REPLAY_SCHEMA
        || header.game_id != row.game_id
        || header.context_id != row.context_id
        || header.index_ref != row.visibility_index_ref
    {
        return Err(fault("header"));
    }
    Ok(())
}
/// Index bound to its manifest, header and original opening; returns whether
/// the archive concludes natively (else it is an operational prefix).
fn verify_index(
    row: &ManifestRow,
    header: &EncodedHeader,
    index: &EncodedIndex,
    opening: &OpeningFacts,
) -> Result<bool, NativeSourceError> {
    if index.version != REPLAY_SCHEMA
        || index.game_id != row.game_id
        || index.context_id != row.context_id
        || index.release_id != header.release_id
        || index.initial_checkpoint_id != row.initial_checkpoint_id
        || index.through_event_sequence != row.through_event_sequence
        || index.through_revision != row.through_accepted_revision
    {
        return Err(fault("index"));
    }
    if (opening.game, opening.context) != (row.game_id, row.context_id)
        || index.lineage != opening.lineage
        || index.opening_receipt_id != opening.opening_receipt_id
        || index.initial_checkpoint_id != opening.initial_checkpoint_id
    {
        return Err(fault("lineage"));
    }
    match index.conclusion.as_str() {
        NATIVE_CONCLUSION => Ok(true),
        OPERATIONAL_INCIDENT => Ok(false),
        _ => Err(fault("conclusion")),
    }
}
/// The one perspective entry for this seat and disclosure, with a complete
/// contiguous page-range table from zero to the frontier. None means the seat
/// is not retained with that disclosure.
fn select_perspective(
    index: &EncodedIndex,
    perspective: Uuid,
    disclosure: NativeDisclosure,
    native_conclusion: bool,
) -> Result<Option<&EncodedPerspective>, NativeSourceError> {
    // An operational prefix never carries full-information pages.
    if disclosure == NativeDisclosure::CompletedParticipant && !native_conclusion {
        return Err(fault("incident full reveal"));
    }
    let wanted = label(disclosure);
    let mut matching = index
        .perspectives
        .iter()
        .filter(|p| p.seat_id == perspective && p.disclosure == wanted);
    let entry = match (matching.next(), matching.next()) {
        (Some(entry), None) => entry,
        (None, _) => return Ok(None),
        (Some(_), Some(_)) => return Err(fault("duplicate perspective")),
    };
    if entry.pages.is_empty() || entry.pages.len() > MAX_PAGES {
        return Err(fault("perspective pages"));
    }
    let mut next = Amount::from(0);
    for page_ref in &entry.pages {
        if page_ref.first != next || page_ref.last < page_ref.first {
            return Err(fault("page ranges"));
        }
        next = page_ref
            .last
            .checked_add(&Amount::from(1))
            .map_err(|_| fault("page ranges"))?;
    }
    if entry.pages.last().map(|page| &page.last) != Some(&index.through_event_sequence) {
        return Err(fault("page ranges"));
    }
    Ok(Some(entry))
}

#[cfg(test)]
mod tests {
    //! Codec and budget shape checks over synthetic bytes. Synthetic references
    //! do not certify artifact bytes; real archives are covered by the owned
    //! connected replay runs.
    use super::*;
    use serde_json::{Value, json};

    fn id(n: u8) -> Uuid {
        Uuid::from_bytes([n; 16])
    }
    fn artifact(role: &str) -> String {
        format!(
            "native-artifact-v1/{}/{}/{role}/1/{}.json",
            id(1),
            id(2),
            "a".repeat(64)
        )
    }
    fn header() -> Value {
        json!({"version":REPLAY_SCHEMA,"gameId":id(1),"contextId":id(2),"releaseId":id(3),
            "schemaRevision":REPLAY_SCHEMA,"indexRef":artifact("replay")})
    }
    fn lineage() -> Value {
        json!({"nativeEpoch":"42","runtimeId":"b".repeat(64),"programSourceId":"c".repeat(64),
            "qualificationRef":"fixture:qualified","initialRef":artifact("initial"),
            "initialStateRef":artifact("state")})
    }
    fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, serde_json::Error> {
        crate::decode_wire_object(&serde_json::to_vec(value).unwrap())
    }

    #[test]
    fn header_codec_is_strict() {
        let decoded: EncodedHeader = decode(&header()).unwrap();
        assert!(decoded.index_ref.role() == NativeArtifactRole::Replay);
        let mut unknown = header();
        unknown["extra"] = json!(true);
        assert!(decode::<EncodedHeader>(&unknown).is_err());
        let mut missing = header();
        missing.as_object_mut().unwrap().remove("indexRef");
        assert!(decode::<EncodedHeader>(&missing).is_err());
        assert!(decode::<EncodedHeader>(&json!([header()])).is_err());
        // A complete header with one key repeated: only the duplicate is wrong.
        let complete = serde_json::to_string(&header()).unwrap();
        let duplicate = format!(
            "{},\"version\":\"{REPLAY_SCHEMA}\"}}",
            complete.strip_suffix('}').unwrap()
        );
        assert!(crate::decode_wire_object::<EncodedHeader>(complete.as_bytes()).is_ok());
        assert!(crate::decode_wire_object::<EncodedHeader>(duplicate.as_bytes()).is_err());
        // A syntactically malformed reference fails decoding; role checks are
        // the reader's, after decoding.
        let mut malformed = header();
        malformed["indexRef"] = json!("not-an-artifact");
        assert!(decode::<EncodedHeader>(&malformed).is_err());
    }

    #[test]
    fn lineage_codec_is_strict_and_binding() {
        let original: EncodedLineage = decode(&lineage()).unwrap();
        assert!(original == decode::<EncodedLineage>(&lineage()).unwrap());
        let mut other_runtime = lineage();
        other_runtime["runtimeId"] = json!("d".repeat(64));
        assert!(original != decode::<EncodedLineage>(&other_runtime).unwrap());
        let mut unknown = lineage();
        unknown["currentRuntime"] = json!("b".repeat(64));
        assert!(decode::<EncodedLineage>(&unknown).is_err());
        let mut numeric = lineage();
        numeric["nativeEpoch"] = json!(42);
        assert!(decode::<EncodedLineage>(&numeric).is_err());
    }

    #[test]
    fn aggregate_budget_is_exact_and_refuses_overflow() {
        let template = NativeRecoveryBudget::new(2, 10).unwrap();
        assert!(scaled(&template, 0).is_err());
        assert!(scaled(&NativeRecoveryBudget::new(usize::MAX, 1).unwrap(), 2).is_err());
        assert!(scaled(&NativeRecoveryBudget::new(1, u64::MAX).unwrap(), 2).is_err());
        let mut budget = scaled(&template, 3).unwrap();
        let reference: NativeArtifactRef =
            serde_json::from_value(json!(artifact("replay"))).unwrap();
        assert_eq!(reference.size(), 1);
        // Three templates of two records each: six charges fit, a seventh does not.
        for _ in 0..6 {
            charge(&mut budget, &reference).unwrap();
        }
        assert!(charge(&mut budget, &reference).is_err());
        let mut single = scaled(&NativeRecoveryBudget::new(1, 1).unwrap(), 1).unwrap();
        charge(&mut single, &reference).unwrap();
        assert!(charge(&mut single, &reference).is_err());
    }

    mod retention {
        //! Loopback Store mechanics only; no native rules or hosted qualification.
        use super::*;
        use crate::{
            native_game::artifacts::{MAX_NATIVE_ARTIFACT_BYTES, NativeArtifactFailure},
            test_http::{Mock, Step},
        };

        const BYTES: &[u8] = b"retained-replay-fixture";

        fn store(mock: &Mock) -> NativeArtifactStore {
            NativeArtifactStore::new(
                BackendClient::new(
                    BackendConfig::new(&mock.base, "sb_secret_test_only").unwrap(),
                    "replay-retention-test",
                    Duration::from_secs(1),
                )
                .unwrap(),
                "native-evidence".into(),
            )
            .unwrap()
        }
        fn reference() -> NativeArtifactRef {
            NativeArtifactRef::for_bytes(id(1), id(2), NativeArtifactRole::Replay, BYTES).unwrap()
        }
        fn bucket() -> Step {
            Step::get(
                "/storage/v1/bucket/native-evidence".into(),
                200,
                br#"{"id":"native-evidence","public":false}"#.to_vec(),
            )
        }
        fn get(status: u16, bytes: &[u8]) -> Step {
            Step::get(
                format!(
                    "/storage/v1/object/authenticated/native-evidence/{}",
                    reference().as_str()
                ),
                status,
                bytes.to_vec(),
            )
        }
        fn upload(status: u16) -> Step {
            let mut step = Step::json(
                &format!(
                    "/storage/v1/object/native-evidence/{}",
                    reference().as_str()
                ),
                json!({}),
            );
            step.status = status;
            step.expect = Some(BYTES.to_vec());
            step
        }
        fn assert_budget(error: &NativeSourceError) {
            assert!(matches!(
                error.0,
                Failure::Checkpoint(NativeCheckpointError::Budget)
            ));
            assert!(matches!(
                std::error::Error::source(error)
                    .unwrap()
                    .downcast_ref::<NativeCheckpointError>(),
                Some(NativeCheckpointError::Budget)
            ));
        }
        fn assert_artifact(error: &NativeSourceError, expected: NativeArtifactFailure) {
            let Failure::Artifact(artifact) = &error.0 else {
                panic!("expected typed artifact failure");
            };
            assert_eq!(artifact.failure(), expected);
            assert_eq!(
                std::error::Error::source(error)
                    .unwrap()
                    .downcast_ref::<NativeArtifactError>()
                    .unwrap()
                    .failure(),
                expected
            );
        }

        #[test]
        fn insufficient_bytes_or_records_refuse_before_store_io() {
            let size = reference().size();
            let mut short = NativeRecoveryBudget::new(1, size - 1).unwrap();
            let mut no_records = NativeRecoveryBudget::new(1, size).unwrap();
            no_records.charge_work(1, 0).unwrap();
            let mock = Mock::new(vec![]);
            let store = store(&mock);
            for budget in [&mut short, &mut no_records] {
                let before = budget.remaining();
                let error = retain(&store, budget, (id(1), id(2)), BYTES).err().unwrap();
                assert_budget(&error);
                assert_eq!(budget.remaining(), before);
            }
            assert_eq!(mock.finish().len(), 0);
        }

        #[test]
        fn invalid_scope_empty_and_oversize_refuse_before_store_io() {
            let huge = vec![0; usize::try_from(MAX_NATIVE_ARTIFACT_BYTES + 1).unwrap()];
            let mock = Mock::new(vec![]);
            let store = store(&mock);
            let mut budget = NativeRecoveryBudget::new(1, MAX_NATIVE_ARTIFACT_BYTES).unwrap();
            for (scope, bytes, expected) in [
                (
                    (Uuid::nil(), id(2)),
                    BYTES,
                    NativeArtifactFailure::InvalidReference,
                ),
                (
                    (id(1), Uuid::nil()),
                    BYTES,
                    NativeArtifactFailure::InvalidReference,
                ),
                ((id(1), id(2)), b"".as_slice(), NativeArtifactFailure::Size),
                ((id(1), id(2)), huge.as_slice(), NativeArtifactFailure::Size),
            ] {
                let before = budget.remaining();
                let error = retain(&store, &mut budget, scope, bytes).err().unwrap();
                assert_artifact(&error, expected);
                assert_eq!(budget.remaining(), before);
            }
            assert_eq!(mock.finish().len(), 0);
        }

        #[test]
        fn exact_allowance_retains_matching_bytes_and_charges_once() {
            let mock = Mock::new(vec![
                bucket(),
                get(404, b"missing"),
                upload(200),
                get(200, BYTES),
            ]);
            let store = store(&mock);
            let mut budget = NativeRecoveryBudget::new(1, reference().size()).unwrap();
            assert!(retain(&store, &mut budget, (id(1), id(2)), BYTES).unwrap() == reference());
            assert_eq!(budget.remaining(), (0, 0));
            assert_budget(
                &retain(&store, &mut budget, (id(1), id(2)), BYTES)
                    .err()
                    .unwrap(),
            );
            let seen = mock.finish();
            assert_eq!(seen.len(), 4);
            assert_eq!(seen[2].2, BYTES);
            assert_eq!(seen[2].1.get("x-upsert").map(String::as_str), Some("false"));
        }

        #[test]
        fn deduplicated_retention_still_charges_each_attempt() {
            let mock = Mock::new(vec![bucket(), get(200, BYTES), get(200, BYTES)]);
            let store = store(&mock);
            let size = reference().size();
            let mut budget = NativeRecoveryBudget::new(2, size * 2).unwrap();
            for expected in [(1, size), (0, 0)] {
                assert!(retain(&store, &mut budget, (id(1), id(2)), BYTES).unwrap() == reference());
                assert_eq!(budget.remaining(), expected);
            }
            assert_budget(
                &retain(&store, &mut budget, (id(1), id(2)), BYTES)
                    .err()
                    .unwrap(),
            );
            assert_eq!(mock.finish().len(), 3);
        }

        #[test]
        fn storage_failure_keeps_cause_and_spent_reservation() {
            let mock = Mock::new(vec![
                bucket(),
                get(404, b"missing"),
                upload(503),
                get(404, b"missing"),
            ]);
            let store = store(&mock);
            let mut budget = NativeRecoveryBudget::new(1, reference().size()).unwrap();
            let error = retain(&store, &mut budget, (id(1), id(2)), BYTES)
                .err()
                .unwrap();
            assert_artifact(&error, NativeArtifactFailure::Storage);
            let artifact = std::error::Error::source(&error).unwrap();
            let storage = artifact.source().unwrap();
            assert!(storage.source().is_some());
            assert_eq!(budget.remaining(), (0, 0));
            assert_budget(
                &retain(&store, &mut budget, (id(1), id(2)), BYTES)
                    .err()
                    .unwrap(),
            );
            assert_eq!(mock.finish().len(), 4);
        }

        #[test]
        fn mismatched_store_readback_refuses_and_keeps_spent_reservation() {
            let corrupt = vec![b'x'; BYTES.len()];
            let mock = Mock::new(vec![
                bucket(),
                get(404, b"missing"),
                upload(200),
                get(200, &corrupt),
            ]);
            let store = store(&mock);
            let mut budget = NativeRecoveryBudget::new(1, reference().size()).unwrap();
            let error = retain(&store, &mut budget, (id(1), id(2)), BYTES)
                .err()
                .unwrap();
            assert_artifact(&error, NativeArtifactFailure::Storage);
            assert!(
                std::error::Error::source(&error)
                    .unwrap()
                    .source()
                    .is_some()
            );
            assert_eq!(budget.remaining(), (0, 0));
            assert_eq!(mock.finish().len(), 4);
        }
    }

    /// A named in-place corruption of a fixture.
    type Tamper<T> = (&'static str, fn(&mut T));
    /// A consistent synthetic manifest/header/index/opening, one seat with two
    /// pages (0..=63, 64..=70) per disclosure.
    struct Archive {
        row: ManifestRow,
        header: EncodedHeader,
        index: EncodedIndex,
        opening: OpeningFacts,
    }
    fn archive() -> Archive {
        let replay = |n: u8| -> NativeArtifactRef {
            serde_json::from_value(json!(format!(
                "native-artifact-v1/{}/{}/replay/1/{}.json",
                id(1),
                id(2),
                char::from(b'a' + n).to_string().repeat(64)
            )))
            .unwrap()
        };
        let pages = || {
            vec![
                EncodedPageRef {
                    first: Amount::from(0),
                    last: Amount::from(63),
                    page_ref: replay(3),
                },
                EncodedPageRef {
                    first: Amount::from(64),
                    last: Amount::from(70),
                    page_ref: replay(4),
                },
            ]
        };
        let perspective = |disclosure: &str| EncodedPerspective {
            seat_id: id(9),
            disclosure: disclosure.to_owned(),
            pages: pages(),
        };
        let index = EncodedIndex {
            version: REPLAY_SCHEMA.to_owned(),
            lineage: decode(&lineage()).unwrap(),
            game_id: id(1),
            context_id: id(2),
            release_id: id(3),
            opening_receipt_id: id(4),
            initial_checkpoint_id: id(5),
            through_event_sequence: Amount::from(70),
            through_revision: Amount::from(12),
            vocabulary_ref: replay(5),
            vocabulary_id: serde_json::from_value(json!("e".repeat(64))).unwrap(),
            program_identity_ref: replay(0),
            conclusion: NATIVE_CONCLUSION.to_owned(),
            perspectives: vec![perspective("live"), perspective("completed")],
        };
        Archive {
            row: ManifestRow {
                id: id(6),
                game_id: id(1),
                context_id: id(2),
                artifact_ref: replay(1),
                visibility_index_ref: replay(2),
                schema_revision: REPLAY_SCHEMA.to_owned(),
                initial_checkpoint_id: id(5),
                through_event_sequence: Amount::from(70),
                through_accepted_revision: Amount::from(12),
                source_receipt_id: id(7),
                qualification_ref: qualification(&replay(1)),
            },
            header: EncodedHeader {
                version: REPLAY_SCHEMA.to_owned(),
                game_id: id(1),
                context_id: id(2),
                release_id: id(3),
                schema_revision: REPLAY_SCHEMA.to_owned(),
                index_ref: replay(2),
            },
            index,
            opening: OpeningFacts {
                game: id(1),
                context: id(2),
                lineage: decode(&lineage()).unwrap(),
                opening_receipt_id: id(4),
                initial_checkpoint_id: id(5),
            },
        }
    }
    fn verified(a: &Archive) -> Result<bool, NativeSourceError> {
        verify_header(&a.row, &a.header)?;
        verify_index(&a.row, &a.header, &a.index, &a.opening)
    }

    #[test]
    fn consistent_archive_verifies() {
        let a = archive();
        assert!(verified(&a).unwrap());
        for disclosure in [
            NativeDisclosure::LiveParticipant,
            NativeDisclosure::CompletedParticipant,
        ] {
            assert!(
                select_perspective(&a.index, id(9), disclosure, true)
                    .unwrap()
                    .is_some()
            );
        }
        assert!(
            select_perspective(&a.index, id(8), NativeDisclosure::LiveParticipant, true)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn tampered_header_or_index_is_refused() {
        let tampers: Vec<Tamper<Archive>> = vec![
            ("header version", |a| {
                a.header.version = "spellwake-replay-v0".into()
            }),
            ("header schema", |a| {
                a.header.schema_revision = "other".into()
            }),
            ("header game", |a| a.header.game_id = id(42)),
            ("header scope", |a| a.header.context_id = id(42)),
            ("header index ref", |a| {
                a.header.index_ref = a.row.artifact_ref.clone()
            }),
            ("index version", |a| {
                a.index.version = "spellwake-replay-unsupported".into()
            }),
            ("index scope", |a| a.index.game_id = id(42)),
            ("index context", |a| a.index.context_id = id(42)),
            ("index release", |a| a.index.release_id = id(42)),
            ("index checkpoint", |a| {
                a.index.initial_checkpoint_id = id(42)
            }),
            ("index frontier", |a| {
                a.index.through_event_sequence = Amount::from(71)
            }),
            ("index revision", |a| {
                a.index.through_revision = Amount::from(13)
            }),
            ("conclusion", |a| a.index.conclusion = "native".into()),
            ("opening receipt", |a| a.index.opening_receipt_id = id(42)),
            ("opening scope", |a| a.opening.context = id(42)),
            ("lineage runtime", |a| {
                a.index.lineage.runtime_id = "f".repeat(64)
            }),
            ("lineage program", |a| {
                a.index.lineage.program_source_id = "f".repeat(64)
            }),
            ("lineage epoch", |a| {
                a.index.lineage.native_epoch = serde_json::from_value(json!("43")).unwrap()
            }),
            ("lineage proof", |a| {
                a.index.lineage.qualification_ref = "fixture:other".into()
            }),
            ("lineage initial state", |a| {
                a.index.lineage.initial_state_ref = serde_json::from_value(json!(
                    artifact("state").replace(&"a".repeat(64), &"b".repeat(64))
                ))
                .unwrap();
            }),
            ("lineage initial", |a| {
                a.index.lineage.initial_ref = a.index.lineage.initial_state_ref.clone()
            }),
        ];
        for (name, tamper) in tampers {
            let mut a = archive();
            tamper(&mut a);
            assert!(verified(&a).is_err(), "{name} must be refused");
        }
    }

    #[test]
    fn page_table_and_perspective_faults_are_refused() {
        let live = NativeDisclosure::LiveParticipant;
        let tampers: Vec<Tamper<EncodedIndex>> = vec![
            ("gap", |i| {
                i.perspectives[0].pages[1].first = Amount::from(65)
            }),
            ("overlap", |i| {
                i.perspectives[0].pages[1].first = Amount::from(63)
            }),
            ("not from zero", |i| {
                i.perspectives[0].pages[0].first = Amount::from(1)
            }),
            ("inverted", |i| {
                i.perspectives[0].pages[1].last = Amount::from(60)
            }),
            ("short of frontier", |i| {
                i.perspectives[0].pages[1].last = Amount::from(69)
            }),
            ("empty", |i| i.perspectives[0].pages.clear()),
            ("duplicate", |i| {
                let copy = EncodedPerspective {
                    seat_id: i.perspectives[0].seat_id,
                    disclosure: i.perspectives[0].disclosure.clone(),
                    pages: Vec::new(),
                };
                i.perspectives.push(copy);
            }),
        ];
        for (name, tamper) in tampers {
            let mut a = archive();
            tamper(&mut a.index);
            assert!(
                select_perspective(&a.index, id(9), live, true).is_err(),
                "{name} must be refused"
            );
        }
        // An operational prefix never yields full-information pages.
        let mut incident = archive();
        incident.index.conclusion = OPERATIONAL_INCIDENT.to_owned();
        assert!(!verified(&incident).unwrap());
        assert!(
            select_perspective(
                &incident.index,
                id(9),
                NativeDisclosure::CompletedParticipant,
                false
            )
            .is_err()
        );
        assert!(
            select_perspective(&incident.index, id(9), live, false)
                .unwrap()
                .is_some()
        );
    }
}
