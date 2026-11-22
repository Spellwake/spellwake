//! Accepted historical cut projections, separate from current command authority.
use super::rpc::RecordBudget;
use super::*;
use crate::player_wire::PlayerTerminal;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, sync_channel},
    },
    thread,
};

/// One verified SQL-winning historical observation. No mutable projection or
/// private command registry escapes the walk. This is not HTTP delivery authority.
pub struct NativeHistoricalObservation {
    pub(super) receipt_id: Uuid,
    pub(super) checkpoint_id: Uuid,
    pub(super) creator_receipt_id: Uuid,
    pub(super) view_ref: NativeArtifactRef,
    pub(super) event_id: Uuid,
    pub(super) event_ref: NativeArtifactRef,
    pub(super) state_ref: NativeArtifactRef,
    pub(super) projection: NativeProjectedSnapshot,
    pub(super) terminal: Option<PlayerTerminal>,
}
impl NativeHistoricalObservation {
    /// Original SQL View receipt, for private archive/cursor composition only.
    pub fn receipt_id(&self) -> Uuid {
        self.receipt_id
    }
    /// Accepted SQL checkpoint identity, not a private artifact locator.
    pub fn checkpoint_id(&self) -> Uuid {
        self.checkpoint_id
    }
    /// Canonical accepted creator source independently selected by SQL.
    pub fn creator_receipt_id(&self) -> Uuid {
        self.creator_receipt_id
    }
    /// Private SQL-winning View artifact for retained lineage, never a client URL.
    pub fn view_reference(&self) -> &NativeArtifactRef {
        &self.view_ref
    }
    /// Actual verified projection and its independent small metadata.
    pub fn projection(&self) -> &NativeProjectedSnapshot {
        &self.projection
    }
    /// Native conclusion bound to this cut's seats; None before the game ends.
    /// Operational no-contest is never inferred here.
    pub fn terminal(&self) -> Option<&PlayerTerminal> {
        self.terminal.as_ref()
    }
}
/// Complete original native walk and historical observations. Current actor and
/// source renewal are still required before any public publication. A failed walk
/// returns no collection, even if private immutable Views were issued along it.
pub struct NativeHumanHistory {
    recovered: RecoveredNativeGame,
    observations: Vec<NativeHistoricalObservation>,
    remaining_budget: NativeRecoveryBudget,
    disclosure: NativeDisclosure,
}
impl NativeHumanHistory {
    /// Disclosure established by this producer, never selected by a cursor body.
    pub fn disclosure(&self) -> NativeDisclosure {
        self.disclosure
    }
    /// Actual final accepted cut, not a command lease or completed-reveal permit.
    pub fn recovered(&self) -> &RecoveredNativeGame {
        &self.recovered
    }
    /// Complete ordered seat-filtered cut observations, including baseline zero.
    pub fn observations(&self) -> &[NativeHistoricalObservation] {
        &self.observations
    }
    /// Unused aggregate budget left after this walk, for further page/archive work.
    pub(super) fn remaining_budget(&self) -> NativeRecoveryBudget {
        self.remaining_budget.clone()
    }
}

/// Borrowed accepted cut. Construction occurs only after HistoryReplay observed
/// the actual checkpoint and verified its retained native Event/result bytes.
pub(in crate::native_game) struct HistoricalCut<'a> {
    checkpoint: &'a NativeCheckpoint,
    binding: NativeBinding,
    opening_receipt_id: Uuid,
    checkpoint_id: Uuid,
    creator_receipt_id: Uuid,
    through: Amount,
    event_id: Uuid,
    event_ref: NativeArtifactRef,
    source_anchor: NativeArtifactRef,
    full: bool,
}
/// Verified cut metadata and an owned reference to its immutable native state.
struct ArchiveViewJob {
    perspective: usize,
    cut_index: usize,
    checkpoint: Arc<NativeCheckpoint>,
    binding: NativeBinding,
    opening_receipt_id: Uuid,
    checkpoint_id: Uuid,
    creator_receipt_id: Uuid,
    through: Amount,
    event_id: Uuid,
    event_ref: NativeArtifactRef,
    source_anchor: NativeArtifactRef,
    full: bool,
}

impl ArchiveViewJob {
    fn new(
        perspective: usize,
        cut_index: usize,
        checkpoint: Arc<NativeCheckpoint>,
        cut: HistoricalCut<'_>,
    ) -> Self {
        Self {
            perspective,
            cut_index,
            checkpoint,
            binding: cut.binding,
            opening_receipt_id: cut.opening_receipt_id,
            checkpoint_id: cut.checkpoint_id,
            creator_receipt_id: cut.creator_receipt_id,
            through: cut.through,
            event_id: cut.event_id,
            event_ref: cut.event_ref,
            source_anchor: cut.source_anchor,
            full: cut.full,
        }
    }

    fn render(
        self,
        repository: &NativeSourceRepository,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        proof: &HistoricalCompletionAuthority,
        budget: RecordBudget<'_>,
    ) -> Result<NativeHistoricalObservation, NativeSourceError> {
        let provenance = self.checkpoint.provenance().ok_or_else(invalid)?;
        let display = NativeImmutableDisplay::new(programs, provenance)
            .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        let cut = HistoricalCut {
            checkpoint: &self.checkpoint,
            binding: self.binding,
            opening_receipt_id: self.opening_receipt_id,
            checkpoint_id: self.checkpoint_id,
            creator_receipt_id: self.creator_receipt_id,
            through: self.through,
            event_id: self.event_id,
            event_ref: self.event_ref,
            source_anchor: self.source_anchor,
            full: self.full,
        };
        repository.project_verified_cut(store, cut, &display, Some(proof), budget, true)
    }
}

/// Which historical authority a walk runs under.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HistoricalKind {
    /// Native conclusion + SQL completion, full reveal.
    CompletedFull,
    /// Native conclusion + SQL completion, any retained seat filtered.
    #[cfg(test)]
    CompletedSeat,
    /// Accepted operational no-contest before any native conclusion, any
    /// retained seat filtered. Never full reveal.
    IncidentSeat,
}
/// Original terminal truth plus SQL participant completion permission, or an
/// accepted operational no-contest classification (seat-filtered only). Neither
/// serializable nor externally constructible, and never a session lifetime lease.
pub(in crate::native_game) struct HistoricalCompletionAuthority {
    access: NativeHumanAccess,
    initial_ref: NativeArtifactRef,
    terminal_ref: NativeArtifactRef,
    full_allowed: bool,
}

/// The verified terminal observation shared by one portable archive build.
pub(super) struct ArchiveCompletionAuthority {
    proof: HistoricalCompletionAuthority,
    pub(super) proof_records: usize,
    pub(super) proof_bytes: u64,
    pub(super) humans: Vec<Uuid>,
    pub(super) roster: Vec<Uuid>,
}

pub(super) struct ArchivePerspective {
    pub(super) seat: Uuid,
    pub(super) disclosure: NativeDisclosure,
    pub(super) observations: Vec<NativeHistoricalObservation>,
}

pub(super) struct ArchiveHistory {
    pub(super) recovered: RecoveredNativeGame,
    pub(super) perspectives: Vec<ArchivePerspective>,
}

impl ArchiveCompletionAuthority {
    pub(super) fn charge_recovery(
        &self,
        budget: &mut NativeRecoveryBudget,
        perspectives: usize,
    ) -> Result<(), NativeSourceError> {
        // Preserve the existing logical proof + recall charge per view, although
        // the native walks now serve the whole archive.
        let walks = perspectives.checked_mul(2).ok_or_else(invalid)?;
        let records = self.proof_records.checked_mul(walks).ok_or_else(invalid)?;
        let bytes = self
            .proof_bytes
            .checked_mul(u64::try_from(walks).map_err(|_| invalid())?)
            .ok_or_else(invalid)?;
        budget
            .charge_work(records, bytes)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))
    }
}
impl HistoricalCompletionAuthority {
    fn from_recovered(
        access: &NativeHumanAccess,
        recovered: &RecoveredNativeGame,
        kind: HistoricalKind,
    ) -> Result<Self, NativeSourceError> {
        let native_conclusion = recovered.checkpoint().state().outcome().is_some();
        let allowed = match kind {
            HistoricalKind::CompletedFull => {
                access.completed_disclosure().is_some() && native_conclusion
            }
            #[cfg(test)]
            HistoricalKind::CompletedSeat => {
                access.completed_disclosure().is_some() && native_conclusion
            }
            HistoricalKind::IncidentSeat => access.operational_no_contest() && !native_conclusion,
        };
        if !allowed
            || !access.matches_source(recovered)
            || !recovered.history_evidence().transitions_verified()
        {
            return Err(invalid());
        }
        Ok(Self {
            access: access.clone(),
            initial_ref: recovered.checkpoint().initial_reference().clone(),
            terminal_ref: recovered.checkpoint().reference().clone(),
            full_allowed: kind == HistoricalKind::CompletedFull,
        })
    }
    pub(in crate::native_game) fn permits(&self, cut: &HistoricalCut<'_>) -> bool {
        let basis = cut.checkpoint.basis();
        let terminal = self.access.basis();
        self.full_allowed
            && cut.full
            && cut.source_anchor == self.terminal_ref
            && cut.checkpoint.initial_reference() == &self.initial_ref
            && basis.game_id == terminal.game_id
            && basis.context_id == terminal.context_id
            && basis.epoch == terminal.epoch
            && basis.revision <= terminal.revision
            && &cut.through <= self.access.frontier()
    }
}
impl<'a> HistoricalCut<'a> {
    pub(in crate::native_game) fn projection_source(
        &self,
    ) -> (&'a GameState, &NativeBinding, &Amount) {
        (self.checkpoint.state(), &self.binding, &self.through)
    }
    fn from_verified_observation(
        access: &NativeHumanAccess,
        source: &NativeGameSource,
        opening: &NativeOpeningSource,
        group: &super::history::Group,
        checkpoint: &'a NativeCheckpoint,
        perspective: Uuid,
        completion: Option<(&HistoricalCompletionAuthority, bool)>,
    ) -> Result<Self, NativeSourceError> {
        let full = completion.is_some_and(|(_, full)| full);
        let completion = completion.map(|(authority, _)| authority);
        if full && completion.is_some_and(|authority| !authority.full_allowed) {
            return Err(invalid());
        }
        let [event] = group.events.as_slice() else {
            return Err(invalid());
        };
        let basis = checkpoint.basis();
        if let Some(completion) = completion {
            if !completion.access.same_delivery_source(access) {
                return Err(invalid());
            }
        } else if perspective != access.viewer_seat_id() {
            return Err(invalid());
        }
        if !access.matches_source_metadata(source, opening)
            || group.event_schema_revision.is_none()
            || group.accepted_revision != basis.revision
            || group.state_ref != *checkpoint.reference()
            || checkpoint.initial_reference() != opening.artifacts().initial()
            || basis.game_id != source.game_id()
            || basis.context_id != source.context_ids().0
            || basis.epoch != opening.native_epoch()
            || basis.revision > access.basis().revision
            || event.sequence > *source.last_event_sequence()
            || basis.revision.get() == 0
                && (group.checkpoint_id != opening.receipt_ids().2
                    || group.creator_receipt_id != opening.receipt_ids().1)
        {
            return Err(invalid());
        }
        let binding = checkpoint
            .binding(perspective)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        Ok(Self {
            checkpoint,
            binding,
            opening_receipt_id: opening.receipt_ids().0,
            checkpoint_id: group.checkpoint_id,
            creator_receipt_id: group.creator_receipt_id,
            through: event.sequence.clone(),
            event_id: event.event_id,
            event_ref: event.private_payload_ref.clone(),
            source_anchor: source.record.checkpoint.state_ref.clone(),
            full,
        })
    }
}
impl NativeSourceRepository {
    /// Verify the accepted terminal history once for the archive's perspectives.
    pub(super) fn archive_completion_authority(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        mut budget: NativeRecoveryBudget,
    ) -> Result<Option<ArchiveCompletionAuthority>, NativeSourceError> {
        let kind = if access.completed_disclosure().is_some() {
            HistoricalKind::CompletedFull
        } else if access.operational_no_contest() {
            HistoricalKind::IncidentSeat
        } else {
            return Err(invalid());
        };
        let (initial_records, initial_bytes) = budget.remaining();
        let Some(recovered) = self.recover_observed(
            access.basis().game_id,
            store,
            programs,
            &mut budget,
            &mut |_, _, _, _, _| Ok(()),
        )?
        else {
            return Ok(None);
        };
        let proof = HistoricalCompletionAuthority::from_recovered(access, &recovered, kind)?;
        let binding = recovered
            .checkpoint()
            .binding(access.viewer_seat_id())
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        let (remaining_records, remaining_bytes) = budget.remaining();
        Ok(Some(ArchiveCompletionAuthority {
            humans: recovered.source().human_seats().collect(),
            roster: binding.seats.iter().map(|seat| seat.seat_id).collect(),
            proof,
            proof_records: initial_records
                .checked_sub(remaining_records)
                .ok_or_else(invalid)?,
            proof_bytes: initial_bytes
                .checked_sub(remaining_bytes)
                .ok_or_else(invalid)?,
        }))
    }

    /// Project all archive perspectives from one observed, verified native walk.
    pub(super) fn collect_archive_history(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        authority: &ArchiveCompletionAuthority,
        mut native_budget: NativeRecoveryBudget,
        first_budget: &Mutex<NativeRecoveryBudget>,
        shared_budget: &Mutex<NativeRecoveryBudget>,
    ) -> Result<Option<ArchiveHistory>, NativeSourceError> {
        let proof = &authority.proof;
        let viewer = proof.access.viewer_seat_id();
        let mut perspectives: Vec<_> = authority
            .humans
            .iter()
            .map(|&seat| ArchivePerspective {
                seat,
                disclosure: NativeDisclosure::LiveParticipant,
                observations: Vec::new(),
            })
            .collect();
        if proof.full_allowed {
            perspectives.extend(authority.roster.iter().map(|&seat| ArchivePerspective {
                seat,
                disclosure: NativeDisclosure::CompletedParticipant,
                observations: Vec::new(),
            }));
        }
        let physical_start = native_budget.remaining();
        let failed = AtomicBool::new(false);
        let mut cut_count = 0;
        let (recovered, rendered) = thread::scope(|scope| {
            // One cut can wait in the queue. At most six extra jobs are active,
            // and each job shares the verified native State for its cut.
            let (send, receive) = sync_channel::<ArchiveViewJob>(perspectives.len());
            let queue = Arc::new(Mutex::new(receive));
            let (output, results) = channel();
            let mut workers = Vec::new();
            for _ in 0..perspectives.len().min(6) {
                let queue = Arc::clone(&queue);
                let output = output.clone();
                let failed = &failed;
                let worker = thread::Builder::new()
                    .stack_size(16 * 1024 * 1024)
                    .spawn_scoped(scope, move || {
                        loop {
                            let task = queue.lock().map_err(|_| invalid())?.recv();
                            let Ok(job) = task else { break };
                            if failed.load(Ordering::Relaxed) {
                                continue;
                            }
                            let first =
                                job.binding.viewer == viewer && job.full == proof.full_allowed;
                            let budget = if first { first_budget } else { shared_budget };
                            let index = (job.perspective, job.cut_index);
                            let result = job.render(
                                self,
                                store,
                                programs,
                                proof,
                                RecordBudget::Shared(budget),
                            );
                            if result.is_err() {
                                failed.store(true, Ordering::Relaxed);
                            }
                            output.send((index, result)).map_err(|_| invalid())?;
                        }
                        Ok::<(), NativeSourceError>(())
                    })
                    .map_err(|error| {
                        NativeSourceError(Failure::Backend(BackendError::caused(
                            "replay view worker",
                            error,
                        )))
                    })?;
                workers.push(worker);
            }
            drop(queue);
            drop(output);
            let recovered = self.recover_observed(
                proof.access.basis().game_id,
                store,
                programs,
                &mut native_budget,
                &mut |source, opening, group, checkpoint, _| {
                    if failed.load(Ordering::Relaxed) {
                        return Err(invalid());
                    }
                    let shared = Arc::new(checkpoint.clone());
                    for (index, perspective) in perspectives.iter().enumerate() {
                        let full = perspective.disclosure == NativeDisclosure::CompletedParticipant;
                        let cut = HistoricalCut::from_verified_observation(
                            &proof.access,
                            source,
                            opening,
                            group,
                            checkpoint,
                            perspective.seat,
                            Some((proof, full)),
                        )?;
                        send.send(ArchiveViewJob::new(
                            index,
                            cut_count,
                            Arc::clone(&shared),
                            cut,
                        ))
                        .map_err(|_| invalid())?;
                    }
                    cut_count += 1;
                    Ok(())
                },
            );
            drop(send);
            let mut worker_failure = None;
            for worker in workers {
                if let Err(error) = worker
                    .join()
                    .map_err(|_| invalid())
                    .and_then(|result| result)
                {
                    worker_failure.get_or_insert(error);
                }
            }
            let mut rendered = Vec::new();
            for (index, result) in results {
                rendered.push((index, result?));
            }
            if let Some(error) = worker_failure {
                return Err(error);
            }
            Ok::<_, NativeSourceError>((recovered?, rendered))
        })?;
        let mut rendered = rendered;
        rendered.sort_unstable_by_key(|entry| entry.0);
        for ((perspective, cut_index), observation) in rendered {
            let part = perspectives.get_mut(perspective).ok_or_else(invalid)?;
            if cut_index != part.observations.len() {
                return Err(invalid());
            }
            part.observations.push(observation);
        }
        if perspectives
            .iter()
            .any(|part| part.observations.len() != cut_count)
        {
            return Err(invalid());
        }
        let Some(recovered) = recovered else {
            return Ok(None);
        };
        let physical_end = native_budget.remaining();
        if !proof.access.matches_source(&recovered)
            || !recovered.history_evidence().transitions_verified()
            || physical_start.0.checked_sub(physical_end.0) != Some(authority.proof_records)
            || physical_start.1.checked_sub(physical_end.1) != Some(authority.proof_bytes)
        {
            return Err(invalid());
        }
        Ok(Some(ArchiveHistory {
            recovered,
            perspectives,
        }))
    }

    /// Project the actual admitted human's historical seat over one native walk.
    /// SQL first-winner View issuance determines all public handles. No historical
    /// projection becomes an IssuedNativeHumanView/current command operand.
    ///
    /// Caller must refresh the original actor/session/account and complete source
    /// after composition and retain its deadline through actual HTTP publication.
    /// This ordinary seat-filtered producer does not authorize completed-full
    /// history; that path requires its separate terminal authority.
    ///
    /// # Errors
    /// Rejects stale access/root/cut, missing selected-card provenance, unqualified
    /// legacy/multi-Event history, native byte disagreement, View/registry faults
    /// or aggregate budget exhaustion. None means the selected game disappeared.
    pub fn project_human_history(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        budget: NativeRecoveryBudget,
    ) -> Result<Option<NativeHumanHistory>, NativeSourceError> {
        self.collect_human_history(
            store,
            programs,
            access,
            budget,
            access.viewer_seat_id(),
            None,
        )
    }
    /// Project any retained perspective after actual native and SQL completion.
    /// Earlier cuts use a sealed read-only permit; no terminal State is fabricated.
    /// Fresh original actor/source/deadline checks are still required for delivery.
    /// # Errors
    /// Rejects premature/operational-only completion, foreign perspectives, original
    /// source drift and every strict history/View/budget failure. No partial output.
    pub fn project_completed_history(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        perspective: Uuid,
        budget: NativeRecoveryBudget,
    ) -> Result<Option<NativeHumanHistory>, NativeSourceError> {
        self.project_authorized_history(
            store,
            programs,
            access,
            perspective,
            budget,
            HistoricalKind::CompletedFull,
        )
    }
    /// Seat-filtered history of any retained seat, only after the same actual
    /// native terminal and SQL completion authority as full reveal. A completed
    /// participant may already see every seat in full, so this discloses nothing
    /// further; it exists so one retained archive can serve each seat's own
    /// historical replay. Live games never reach this path.
    /// # Errors
    /// As project_completed_history.
    #[cfg(test)]
    pub(in crate::native_game) fn project_retained_seat_history(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        perspective: Uuid,
        budget: NativeRecoveryBudget,
    ) -> Result<Option<NativeHumanHistory>, NativeSourceError> {
        self.project_authorized_history(
            store,
            programs,
            access,
            perspective,
            budget,
            HistoricalKind::CompletedSeat,
        )
    }
    fn project_authorized_history(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        perspective: Uuid,
        mut budget: NativeRecoveryBudget,
        kind: HistoricalKind,
    ) -> Result<Option<NativeHumanHistory>, NativeSourceError> {
        let precondition = match kind {
            HistoricalKind::IncidentSeat => access.operational_no_contest(),
            _ => access.completed_disclosure().is_some(),
        };
        if !precondition {
            return Err(invalid());
        }
        let recovered = self.recover_observed(
            access.basis().game_id,
            store,
            programs,
            &mut budget,
            &mut |_, _, _, _, _| Ok(()),
        )?;
        let Some(recovered) = recovered else {
            return Ok(None);
        };
        let authority = HistoricalCompletionAuthority::from_recovered(access, &recovered, kind)?;
        recovered
            .checkpoint()
            .binding(perspective)
            .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?;
        // Preserve only original terminal evidence metadata; the second original
        // walk owns its own actual State, not a cloned first-pass State.
        drop(recovered);
        self.collect_human_history(
            store,
            programs,
            access,
            budget,
            perspective,
            Some((&authority, kind == HistoricalKind::CompletedFull)),
        )
    }
    fn collect_human_history(
        &self,
        store: &NativeArtifactStore,
        programs: &NativeProgramSource,
        access: &NativeHumanAccess,
        mut budget: NativeRecoveryBudget,
        perspective: Uuid,
        completion: Option<(&HistoricalCompletionAuthority, bool)>,
    ) -> Result<Option<NativeHumanHistory>, NativeSourceError> {
        let full = completion.is_some_and(|(_, full)| full);
        let disclosure = if full {
            NativeDisclosure::CompletedParticipant
        } else {
            NativeDisclosure::LiveParticipant
        };
        let mut observations = Vec::new();
        let recovered = self.recover_observed(
            access.basis().game_id,
            store,
            programs,
            &mut budget,
            &mut |source, opening, group, checkpoint, budget| {
                let cut = HistoricalCut::from_verified_observation(
                    access,
                    source,
                    opening,
                    group,
                    checkpoint,
                    perspective,
                    completion,
                )?;
                let provenance = checkpoint.provenance().ok_or_else(invalid)?;
                let display = NativeImmutableDisplay::new(programs, provenance)
                    .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
                observations.push(self.project_verified_cut(
                    store,
                    cut,
                    &display,
                    completion.map(|(authority, _)| authority),
                    RecordBudget::Exclusive(budget),
                    false,
                )?);
                Ok(())
            },
        )?;
        let Some(recovered) = recovered else {
            return Ok(None);
        };
        if !access.matches_source(&recovered)
            || !recovered.history_evidence().transitions_verified()
        {
            return Err(invalid());
        }
        Ok(Some(NativeHumanHistory {
            recovered,
            observations,
            remaining_budget: budget,
            disclosure,
        }))
    }
    fn project_verified_cut(
        &self,
        store: &NativeArtifactStore,
        cut: HistoricalCut<'_>,
        display: &dyn NativeDisplaySource,
        completion: Option<&HistoricalCompletionAuthority>,
        budget: RecordBudget<'_>,
        archive: bool,
    ) -> Result<NativeHistoricalObservation, NativeSourceError> {
        let permit = completion
            .filter(|_| cut.full)
            .map(|authority| {
                crate::native_game::projection::HistoricalProjectionPermit::from_cut(
                    authority, &cut,
                )
                .map_err(|error| {
                    NativeSourceError(Failure::View(NativeViewError::Projection(error)))
                })
            })
            .transpose()?;
        let terminal =
            crate::native_game::NativeTerminal::derive(cut.checkpoint.state(), &cut.binding)
                .map_err(|error| NativeSourceError(Failure::Checkpoint(error)))?
                .map(|terminal| PlayerTerminal {
                    outcome: terminal.outcome(),
                    winner_seat_ids: terminal.winner_seat_ids().to_vec(),
                });
        let metadata = NativeProjectionMetadata {
            through_event_sequence: cut.through,
            // Historical cuts have no independently qualified per-decision clock.
            deadline_at: None,
            disclosure: if cut.full {
                NativeDisclosure::CompletedParticipant
            } else {
                NativeDisclosure::LiveParticipant
            },
        };
        let (receipt_id, view) = self.issue_verified_view(
            store,
            super::view::ViewFrame {
                checkpoint: cut.checkpoint,
                binding: cut.binding,
                opening_receipt_id: cut.opening_receipt_id,
                checkpoint_id: cut.checkpoint_id,
                permit,
                archive,
            },
            &metadata,
            display,
            budget,
        )?;
        Ok(NativeHistoricalObservation {
            receipt_id,
            checkpoint_id: cut.checkpoint_id,
            creator_receipt_id: cut.creator_receipt_id,
            view_ref: view.reference().clone(),
            event_id: cut.event_id,
            event_ref: cut.event_ref,
            state_ref: cut.checkpoint.reference().clone(),
            projection: view.into_projection(),
            terminal,
        })
    }
}
