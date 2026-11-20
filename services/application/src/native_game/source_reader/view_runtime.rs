//! Shared native resources for authenticated Views and explicitly owned commands.
//! Runtime/source configuration does not admit a release or activate a game writer.
use super::*;
use crate::{
    actor::RequestActor,
    player_wire::{PlayerSnapshot, PlayerViewRequest},
};
mod commands;
mod incident;
mod match_worker;
mod response_worker;
pub use match_worker::NativeMatchWorker;
pub use response_worker::{NativeResponseJobOutcome, run_owned_response_job};
mod config;
mod controller;
mod history;
mod replay;
pub(crate) use history::NativeHistoryRead;
pub(crate) use replay::NativeReplayRead;
#[cfg(test)]
type DeliveryHook = dyn Fn(
        &NativeSourceRepository,
        Option<&RecoveredNativeGame>,
        &NativeArtifactStore,
        &NativeProgramSource,
    ) + Send
    + Sync;
#[cfg(test)]
type CommandRecoveryHook = dyn Fn(&NativeSourceRepository) + Send + Sync;
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::native_game::source_reader) enum TestDeliveryStage {
    Issue,
    Refresh,
    CommandCommit,
    CommandReject,
    HistoryCompose,
    LiveReplayCompose,
    ArchiveReplayCompose,
}

/// Expected delivery outcomes, separate from malformed sources and transport errors.
pub enum NativeViewOutcome {
    /// Original SQL-issued public projection, without its private handle registry.
    Snapshot(Box<PlayerSnapshot>),
    /// Session was revoked or expired before delivery.
    Unauthorized,
    /// No current participant disclosure exists for this game/context.
    Unavailable,
    /// The accepted cut changed while this read was being composed. Retry a read.
    Stale,
}
/// Crate-private composition result; never a generic public processing API.
pub(crate) enum NativeReadOutcome<T> {
    Ready(T),
    Refused(NativeReadRefusal),
}
/// Refusal carries no public snapshot or private retained source.
pub(crate) enum NativeReadRefusal {
    Unauthorized,
    Unavailable,
    Stale,
}
impl From<NativeReadRefusal> for NativeViewOutcome {
    fn from(value: NativeReadRefusal) -> Self {
        match value {
            NativeReadRefusal::Unauthorized => Self::Unauthorized,
            NativeReadRefusal::Unavailable => Self::Unavailable,
            NativeReadRefusal::Stale => Self::Stale,
        }
    }
}
/// Server-owned frozen source and private Store shared by bounded View/command requests.
/// Deliberately neither Debug nor serializable; distributed clients never own it.
pub struct NativeGameRuntime {
    programs: NativeProgramSource,
    store: NativeArtifactStore,
    recovery: NativeRecoveryBudget,
    #[cfg(test)]
    hook: Option<(TestDeliveryStage, std::sync::Arc<DeliveryHook>)>,
    #[cfg(test)]
    before_command_recovery: Option<std::sync::Arc<CommandRecoveryHook>>,
}
impl NativeGameRuntime {
    pub(crate) fn admission_programs(&self) -> &NativeProgramSource {
        &self.programs
    }
    /// Install already validated immutable inputs and an unused recovery limit template.
    /// This grants no source readiness, participant authority or database capability.
    pub fn new(
        programs: NativeProgramSource,
        store: NativeArtifactStore,
        recovery: NativeRecoveryBudget,
    ) -> Self {
        Self {
            programs,
            store,
            recovery,
            #[cfg(test)]
            hook: None,
            #[cfg(test)]
            before_command_recovery: None,
        }
    }
    #[cfg(test)]
    pub(in crate::native_game::source_reader) fn with_before_command_recovery(
        mut self,
        hook: impl Fn(&NativeSourceRepository) + Send + Sync + 'static,
    ) -> Self {
        self.before_command_recovery = Some(std::sync::Arc::new(hook));
        self
    }
    #[cfg(test)]
    pub(in crate::native_game::source_reader) fn with_test_recovery(
        mut self,
        budget: NativeRecoveryBudget,
    ) -> Self {
        self.recovery = budget;
        self
    }
    #[cfg(test)]
    pub(in crate::native_game::source_reader) fn with_test_store(
        mut self,
        store: NativeArtifactStore,
    ) -> Self {
        self.store = store;
        self
    }
    #[cfg(test)]
    pub(in crate::native_game::source_reader) fn with_delivery_hook(
        self,
        stage: TestDeliveryStage,
        hook: impl Fn(
            &NativeSourceRepository,
            &RecoveredNativeGame,
            &NativeArtifactStore,
            &NativeProgramSource,
        ) + Send
        + Sync
        + 'static,
    ) -> Self {
        self.with_read_delivery_hook(stage, move |repository, recovered, store, programs| {
            hook(
                repository,
                recovered.expect("recovered delivery stage"),
                store,
                programs,
            );
        })
    }
    #[cfg(test)]
    pub(in crate::native_game::source_reader) fn with_read_delivery_hook(
        mut self,
        stage: TestDeliveryStage,
        hook: impl Fn(
            &NativeSourceRepository,
            Option<&RecoveredNativeGame>,
            &NativeArtifactStore,
            &NativeProgramSource,
        ) + Send
        + Sync
        + 'static,
    ) -> Self {
        self.hook = Some((stage, std::sync::Arc::new(hook)));
        self
    }
    #[cfg(test)]
    fn observe_delivery(
        &self,
        stage: TestDeliveryStage,
        repository: &NativeSourceRepository,
        recovered: &RecoveredNativeGame,
    ) {
        self.observe_read_delivery(stage, repository, Some(recovered));
    }
    #[cfg(test)]
    fn observe_read_delivery(
        &self,
        stage: TestDeliveryStage,
        repository: &NativeSourceRepository,
        recovered: Option<&RecoveredNativeGame>,
    ) {
        if let Some((selected, hook)) = &self.hook
            && *selected == stage
        {
            hook(repository, recovered, &self.store, &self.programs);
        }
    }
    fn refresh(
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        request: &PlayerViewRequest,
        previous: &NativeHumanAccess,
        recovered: Option<&RecoveredNativeGame>,
    ) -> Result<Option<NativeReadRefusal>, NativeSourceError> {
        let current = repository.human_access(actor, request.game_id, request.context_id)?;
        classify_refresh(current, previous, recovered)
    }
    /// Deliver only an SQL-issued live participant snapshot with fresh access checks.
    /// Client fields select a game/context only, never viewer, artifact or disclosure.
    /// Cold recovery replays accepted history. No new gameplay input is accepted;
    /// no lease, result, entropy or gameplay clock is written by this delivery.
    ///
    /// # Errors
    /// Rejects invalid request scope, malformed/unavailable source artifacts, foreign
    /// runtime/package, recovery exhaustion, missing provenance or inconsistent Views.
    pub fn view(
        &self,
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        request: &PlayerViewRequest,
    ) -> Result<NativeViewOutcome, NativeSourceError> {
        match self.with_human_view(repository, actor, request, |_, _, issued| {
            Ok(issued.into_snapshot())
        })? {
            NativeReadOutcome::Ready(snapshot) => {
                Ok(NativeViewOutcome::Snapshot(Box::new(snapshot)))
            }
            NativeReadOutcome::Refused(refusal) => Ok(refusal.into()),
        }
    }
    /// Compose only within the actual recovered/SQL-issued View scope. The result
    /// is owned; borrowed native state/registry cannot escape this callback.
    /// Refresh complete access after composition, including on composition errors.
    pub(crate) fn with_human_view<T>(
        &self,
        repository: &NativeSourceRepository,
        actor: &RequestActor,
        request: &PlayerViewRequest,
        compose: impl FnOnce(
            &NativeHumanAccess,
            &RecoveredNativeGame,
            IssuedNativeHumanView<'_>,
        ) -> Result<T, NativeSourceError>,
    ) -> Result<NativeReadOutcome<T>, NativeSourceError> {
        request.validate().map_err(|_| invalid())?;
        let access = match repository.human_access(actor, request.game_id, request.context_id)? {
            NativeHumanAccessOutcome::Granted(access) => access,
            NativeHumanAccessOutcome::Unauthorized => {
                return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unauthorized));
            }
            NativeHumanAccessOutcome::Unavailable => {
                return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unavailable));
            }
        };
        let recovered = match repository.recover(
            request.game_id,
            &self.store,
            &self.programs,
            self.recovery.clone(),
        ) {
            Ok(Some(recovered)) => recovered,
            Ok(None) => return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Unavailable)),
            Err(error) => {
                if let Some(outcome) = Self::refresh(repository, actor, request, &access, None)? {
                    return Ok(NativeReadOutcome::Refused(outcome));
                }
                return Err(error);
            }
        };
        if !access.matches_source(&recovered) {
            return Ok(NativeReadOutcome::Refused(NativeReadRefusal::Stale));
        }
        let provenance = recovered.checkpoint().provenance().ok_or_else(invalid)?;
        let display = NativeImmutableDisplay::new(&self.programs, provenance)
            .map_err(|error| NativeSourceError(Failure::Initial(error)))?;
        let metadata = NativeProjectionMetadata {
            through_event_sequence: recovered.source().last_event_sequence().clone(),
            // A cached View may outlive the operational wait. Bind the deadline at delivery.
            deadline_at: None,
            disclosure: NativeDisclosure::LiveParticipant,
        };
        #[cfg(test)]
        self.observe_delivery(TestDeliveryStage::Issue, repository, &recovered);
        let issued = match repository.issue_human_view(
            &self.store,
            &access,
            &recovered,
            &metadata,
            &display,
        ) {
            Ok(issued) => issued,
            Err(error) => {
                if let Some(outcome) =
                    Self::refresh(repository, actor, request, &access, Some(&recovered))?
                {
                    return Ok(NativeReadOutcome::Refused(outcome));
                }
                return Err(error);
            }
        };
        let composed = issued
            .with_live_deadline(repository, recovered.checkpoint(), access.viewer_seat_id())
            .and_then(|issued| compose(&access, &recovered, issued));
        #[cfg(test)]
        self.observe_delivery(TestDeliveryStage::Refresh, repository, &recovered);
        // Issuance/cache is not an authorization lease. Recheck after all private
        // artifact reads and projection, before returning any public snapshot.
        if let Some(outcome) = Self::refresh(repository, actor, request, &access, Some(&recovered))?
        {
            return Ok(NativeReadOutcome::Refused(outcome));
        }
        composed.map(NativeReadOutcome::Ready)
    }
}
fn classify_refresh(
    current: NativeHumanAccessOutcome,
    previous: &NativeHumanAccess,
    recovered: Option<&RecoveredNativeGame>,
) -> Result<Option<NativeReadRefusal>, NativeSourceError> {
    match current {
        NativeHumanAccessOutcome::Unauthorized => Ok(Some(NativeReadRefusal::Unauthorized)),
        NativeHumanAccessOutcome::Unavailable => Ok(Some(NativeReadRefusal::Unavailable)),
        NativeHumanAccessOutcome::Granted(current) => {
            // A replay manifest published meanwhile changes no game state the
            // composed response depends on; any other change is stale.
            if !previous.same_delivery_source(&current)
                && !previous.same_except_publication(&current)
            {
                return Ok(Some(NativeReadRefusal::Stale));
            }
            if let Some(recovered) = recovered {
                current.bind(recovered)?;
            }
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_classifies_synthetic_policy_outcomes_without_delivering_old_views() {
        // Contract classification only; actual Auth and cut interleavings have
        // separate owned HTTP controls. These are not source-qualified states.
        let previous = NativeHumanAccess::fixture_for_outcome_test(0);
        assert!(matches!(
            classify_refresh(NativeHumanAccessOutcome::Unauthorized, &previous, None).unwrap(),
            Some(NativeReadRefusal::Unauthorized)
        ));
        assert!(matches!(
            classify_refresh(NativeHumanAccessOutcome::Unavailable, &previous, None).unwrap(),
            Some(NativeReadRefusal::Unavailable)
        ));
        assert!(matches!(
            classify_refresh(
                NativeHumanAccessOutcome::Granted(NativeHumanAccess::fixture_for_outcome_test(1)),
                &previous,
                None
            )
            .unwrap(),
            Some(NativeReadRefusal::Stale)
        ));
        assert!(
            classify_refresh(
                NativeHumanAccessOutcome::Granted(NativeHumanAccess::fixture_for_outcome_test(0)),
                &previous,
                None
            )
            .unwrap()
            .is_none()
        );
        // A replay manifest published between the observations is not stale;
        // the same publication plus a revision change is.
        let completed = NativeHumanAccess::fixture_for_publication_test(0, false);
        assert!(
            classify_refresh(
                NativeHumanAccessOutcome::Granted(NativeHumanAccess::fixture_for_publication_test(
                    0, true
                )),
                &completed,
                None
            )
            .unwrap()
            .is_none()
        );
        assert!(matches!(
            classify_refresh(
                NativeHumanAccessOutcome::Granted(NativeHumanAccess::fixture_for_publication_test(
                    1, true
                )),
                &completed,
                None
            )
            .unwrap(),
            Some(NativeReadRefusal::Stale)
        ));
    }
}
