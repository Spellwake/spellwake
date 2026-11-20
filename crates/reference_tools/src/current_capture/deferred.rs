//! Caller-finalized installation for selection11, sharing the native invocation
//! and immediate reexecution used by the predecessor observer.
use super::super::current_profile::CallerCompletion;
use super::*;
use std::{marker::PhantomData, rc::Rc};

// A panic has no native Result to record. A caught unwind must still prevent
// qualifying a capture that contains only a before point and no returned call.
pub(super) struct InvocationGuard(bool);
impl InvocationGuard {
    pub(super) fn new() -> Self {
        Self(
            active(|active| Ok(active.recorder.scope() == CaptureScope::ResolutionSelection11))
                .unwrap_or(false),
        )
    }
    pub(super) fn returned(&mut self) {
        self.0 = false;
    }
}
impl Drop for InvocationGuard {
    fn drop(&mut self) {
        if self.0 {
            remember(Err(
                "native/observer invocation unwound before completion".into()
            ));
        }
    }
}

/// A known caller stop after an already retained native Ok. A retained native Err
/// always closes as NativeError instead, independently of the caller stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonInstallation {
    CallerDeclined,
    RecorderPrepareFailed,
    PolicyAcceptFailed,
    CallerAbortedBeforeInstall,
}
impl NonInstallation {
    fn completion(self) -> CallerCompletion {
        match self {
            Self::CallerDeclined => CallerCompletion::CallerDeclined,
            Self::RecorderPrepareFailed => CallerCompletion::RecorderPrepareFailed,
            Self::PolicyAcceptFailed => CallerCompletion::PolicyAcceptFailed,
            Self::CallerAbortedBeforeInstall => CallerCompletion::CallerAbortedBeforeInstall,
        }
    }
}

/// An actual caller-owned installation slot. Initialization must fill an empty
/// slot; advance must replace the exact source retained before invocation.
pub enum GameStateSlot<'a> {
    Initialize(&'a mut Option<GameState>),
    Advance(&'a mut GameState),
}

/// One non-cloneable completion capability for an already returned root Result.
/// Dropping it without explicit completion prevents capture qualification.
/// It belongs to the thread-local recorder that issued it.
///
/// ```compile_fail
/// use spellwake_reference_tools::current_capture::deferred::CallerTicket;
/// fn transfer(ticket: CallerTicket) {
///     std::thread::spawn(move || drop(ticket));
/// }
/// ```
///
/// ```compile_fail
/// use spellwake_reference_tools::current_capture::deferred::CallerTicket;
/// fn require_sync<T: Sync>() {}
/// require_sync::<CallerTicket>();
/// ```
#[must_use = "complete the caller ticket at actual installation or explicit noninstallation"]
pub struct CallerTicket {
    // ACTIVE and its identities are thread-local. This zero-sized capability
    // marker forbids transferring or sharing the ticket across those owners.
    thread_bound: PhantomData<Rc<()>>,
    origin: Option<(u64, String)>,
    source: Option<GameState>,
    expected: Result<NativeCandidate, GameError>,
    requested: bool,
    completion: Option<CallerCompletion>,
    finished: bool,
}
impl CallerTicket {
    fn new(
        call: Option<String>,
        source: Option<GameState>,
        expected: Result<NativeCandidate, GameError>,
        requested: bool,
    ) -> Self {
        let origin = call.and_then(|call| {
            let binding = active(|active| {
                active.recorder.begin_caller_delivery(&call, requested)?;
                Ok((active.identity, call))
            });
            match binding {
                Ok(origin) => Some(origin),
                Err(error) => {
                    remember(Err(error));
                    None
                }
            }
        });
        Self {
            thread_bound: PhantomData,
            origin,
            source,
            expected,
            requested,
            completion: None,
            finished: false,
        }
    }

    fn with_recorder<T>(
        &self,
        operation: impl FnOnce(&mut Recorder, &str) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        let Some((identity, call)) = &self.origin else {
            return Ok(None);
        };
        active(|active| {
            if active.identity != *identity {
                return Err("caller ticket belongs to another capture".into());
            }
            if active.recorder.scope() != CaptureScope::ResolutionSelection11 {
                return Err("caller ticket has a different capture scope".into());
            }
            operation(&mut active.recorder, call).map(Some)
        })
    }

    fn publish(&mut self) -> Result<(), String> {
        let completion = self
            .completion
            .clone()
            .ok_or("caller outcome was not selected")?;
        let result = self
            .with_recorder(|recorder, call| recorder.complete_caller_delivery(call, completion))
            .map(|_| ());
        self.finished = true;
        if let Err(error) = &result {
            remember(Err(error.clone()));
        }
        result
    }

    /// Complete an explicit caller refusal/failure without installing a state.
    ///
    /// # Errors
    /// Reports stale/foreign/already-completed observer ownership. The original
    /// native Result is not changed. A native Err closes as NativeError.
    pub fn not_installed(mut self, reason: NonInstallation) -> Result<(), String> {
        self.completion = Some(if self.expected.is_err() {
            CallerCompletion::NativeError
        } else {
            reason.completion()
        });
        self.publish()
    }

    /// Install the exact returned candidate, then commit an already prepared
    /// report delta. All fallible report/policy preparation must precede this call.
    ///
    /// The installation and local Installed marker precede `commit`; an unwind
    /// in that callback cannot relabel the installed state as noninstalled.
    ///
    /// # Errors
    /// Before installation, rejects wrong source/candidate, unrequested adoption,
    /// invalid pointer or foreign ticket. A later observer error is sticky and
    /// does not roll back the actual installed state or change the native Result.
    pub fn install(
        self,
        candidate: NativeCandidate,
        slot: GameStateSlot<'_>,
        accepted_record_pointer: &str,
        commit: impl FnOnce(),
    ) -> Result<(), String> {
        self.install_observed(candidate, slot, accepted_record_pointer, |_| commit())
    }

    /// Install through the same checked handoff, lending the actual installed
    /// state to the already prepared recorder/policy commit callback.
    ///
    /// # Errors
    /// Uses the same preinstallation validation and postinstallation sticky
    /// observer-error semantics as [`Self::install`]. No native call is added.
    pub fn install_observed(
        mut self,
        candidate: NativeCandidate,
        slot: GameStateSlot<'_>,
        accepted_record_pointer: &str,
        commit: impl FnOnce(&GameState),
    ) -> Result<(), String> {
        if !self.requested || self.expected.as_ref().ok() != Some(&candidate) {
            return Err(
                "caller installation differs from its requested full native candidate".into(),
            );
        }
        match (&slot, &self.source) {
            (GameStateSlot::Initialize(slot), None) if slot.is_none() => {}
            (GameStateSlot::Advance(slot), Some(source)) if **slot == *source => {}
            _ => {
                return Err(
                    "caller installation slot differs from the retained native source".into(),
                );
            }
        }
        let append = self.with_recorder(|recorder, call| recorder.call_append_pointer(call))?;
        let completion = CallerCompletion::Installed {
            accepted_record_pointer: accepted_record_pointer.into(),
            accepted_append_pointer: append.unwrap_or_default(),
        };
        self.with_recorder(|recorder, call| {
            recorder.validate_caller_completion(call, &completion)
        })?;
        let state = candidate.into_state();
        let installed = match slot {
            GameStateSlot::Initialize(slot) => slot.insert(state),
            GameStateSlot::Advance(slot) => {
                *slot = state;
                slot
            }
        };
        self.completion = Some(completion);
        commit(installed);
        self.publish()
    }
}
impl Drop for CallerTicket {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if self.completion.is_none() {
            self.completion = Some(if self.expected.is_err() {
                CallerCompletion::NativeError
            } else {
                CallerCompletion::CallerAbortedBeforeInstall
            });
        }
        let _ = self.publish();
        if self.origin.is_some() {
            remember(Err(if std::thread::panicking() {
                "caller completion unwound; retained actual installation status"
            } else {
                "caller ticket was dropped without explicit completion"
            }
            .into()));
        }
    }
}

/// Invoke the maintained root call and retain its unchanged Result alongside a
/// one-use caller ticket. Completion performs no native invocation.
pub fn advance_deferred(
    source: &GameState,
    input: GameInput,
    requested: bool,
) -> (Result<NativeCandidate, GameError>, CallerTicket) {
    let (result, call) = advance_call_recorded(source, input, false);
    let ticket = CallerTicket::new(call, Some(source.clone()), result.clone(), requested);
    (result, ticket)
}

fn initialize_deferred_call(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: Option<TurnStart>,
    opening: Option<(spellwake_engine::shuffle::EntropyTape, Vec<u64>)>,
    requested: bool,
) -> (Result<NativeCandidate, GameError>, CallerTicket) {
    let (result, points) = initialize_observed(game, context, seed, start, opening, false);
    let ticket = CallerTicket::new(
        points.map(|points| points.call),
        None,
        result.clone(),
        requested,
    );
    (result, ticket)
}

/// Observe ordinary initialization with the same caller completion as advance.
pub fn initialize_deferred(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    requested: bool,
) -> (Result<NativeCandidate, GameError>, CallerTicket) {
    initialize_deferred_call(game, context, seed, None, None, requested)
}

/// Observe scheduled initialization without claiming installation on native Ok.
pub fn initialize_scheduled_deferred(
    game: GameId,
    context: GameContext,
    seed: GameSeed,
    start: TurnStart,
    requested: bool,
) -> (Result<NativeCandidate, GameError>, CallerTicket) {
    initialize_deferred_call(game, context, seed, Some(start), None, requested)
}

/// Observe opening initialization with the original explicit entropy transcript.
pub fn initialize_opening_deferred(
    game: GameId,
    context: GameContext,
    setup: OpeningSetup,
    entropy_words: Vec<u64>,
    requested: bool,
) -> (Result<NativeCandidate, GameError>, CallerTicket) {
    initialize_deferred_call(
        game,
        context,
        setup.seed,
        None,
        Some((setup.entropy, entropy_words)),
        requested,
    )
}

#[cfg(test)]
#[path = "deferred/tests.rs"]
mod tests;
