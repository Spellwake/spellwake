//! Prepare ordinary observations without publishing accepted history. Existing
//! view/selector/conversion owners are reused; the predecessor delivery order
//! remains in `Recorder::initial` and `Recorder::accepted`.
use super::*;
use crate::current_capture::deferred::{self, CallerTicket, GameStateSlot, NonInstallation};

#[derive(Debug)]
struct SavedNativeCall {
    input: GameInput,
    returned: Result<NativeCandidate, GameError>,
    installed_ordinal: Option<usize>,
}
/// Full original typed returns are separate from committed observer history.
/// Counterfactual births/handles are never added to canonical History by storage.
#[derive(Debug)]
pub(super) struct NativeTrace {
    initialization: Result<NativeCandidate, GameError>,
    initial_installed: bool,
    calls: Vec<SavedNativeCall>,
    replay_verified: bool,
}
impl NativeTrace {
    fn remember_call(&mut self, input: GameInput, returned: Result<NativeCandidate, GameError>) {
        self.replay_verified = false;
        self.calls.push(SavedNativeCall {
            input,
            returned,
            installed_ordinal: None,
        });
    }
    fn replay(&self, fixture: &Fixture, final_state: Option<&GameState>) -> Result<(), String> {
        let initial = crate::current_capture::without_capture(|| initialize_fixture(fixture));
        if initial != self.initialization {
            return Err("saved full native initialization Result differs on replay".into());
        }
        let mut state = if self.initial_installed {
            Some(
                initial
                    .map_err(|_| "native error was marked installed")?
                    .into_state(),
            )
        } else {
            None
        };
        let mut ordinal = 0usize;
        for saved in &self.calls {
            let source = state
                .as_ref()
                .ok_or("saved input has no installed initial state")?;
            let before = source.clone();
            let returned = source.advance(saved.input.clone());
            if source != &before || returned != saved.returned {
                return Err(
                    "saved original native input/full Result or retained source differs on replay"
                        .into(),
                );
            }
            if let Some(installed) = saved.installed_ordinal {
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or("native replay ordinal exhausted")?;
                if installed != ordinal {
                    return Err("native replay installed ordinal differs".into());
                }
                state = Some(
                    returned
                        .map_err(|_| "native error was marked installed")?
                        .into_state(),
                );
            }
        }
        if state.as_ref() != final_state {
            return Err("saved native delivery replay final full state differs".into());
        }
        Ok(())
    }
}

enum TracePosition {
    Initial,
    Advance { index: usize, ordinal: usize },
}

pub(super) fn finalized_capture() -> bool {
    crate::current_capture::scope()
        == Some(crate::current_profile::CaptureScope::ResolutionSelection11)
}
pub(super) fn verify_trace(
    fixture: &Fixture,
    recorder: &mut Recorder,
    state: Option<&GameState>,
) -> Result<(), String> {
    let trace = recorder
        .report
        .native_trace
        .as_mut()
        .ok_or("full native delivery trace missing")?;
    trace.replay_verified = false;
    trace.replay(fixture, state)?;
    trace.replay_verified = true;
    Ok(())
}

fn close_without_install(ticket: CallerTicket, reason: NonInstallation) {
    if let Err(error) = ticket.not_installed(reason) {
        crate::current_capture::retain_observer_failure(error);
    }
}

pub(super) fn initialize_and_install(
    fixture: &Fixture,
    recorder: &mut Recorder,
) -> Result<GameState, String> {
    let (returned, ticket) = match fixture.initialization {
        FixtureInitialization::Opening => deferred::initialize_opening_deferred(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.clone(),
            fixture
                .shuffles
                .iter()
                .flat_map(|recipe| recipe.words.iter().copied())
                .collect(),
            true,
        ),
        FixtureInitialization::Scheduled(start) => deferred::initialize_scheduled_deferred(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.seed.clone(),
            start,
            true,
        ),
    };
    recorder.report.native_trace = Some(NativeTrace {
        initialization: returned.clone(),
        initial_installed: false,
        calls: vec![],
        replay_verified: false,
    });
    let candidate = match returned {
        Ok(candidate) => candidate,
        Err(error) => {
            close_without_install(ticket, NonInstallation::CallerAbortedBeforeInstall);
            verify_trace(fixture, recorder, None)?;
            return Err(format!("initialization failed: {error:?}"));
        }
    };
    let prepared = match recorder.prepare_initial(fixture, &candidate) {
        Ok(prepared) => prepared,
        Err(error) => {
            close_without_install(ticket, NonInstallation::RecorderPrepareFailed);
            verify_trace(fixture, recorder, None)?;
            return Err(error);
        }
    };
    let pointer = prepared.record_pointer().to_owned();
    let mut slot = None;
    let completion = ticket.install_observed(
        candidate,
        GameStateSlot::Initialize(&mut slot),
        &pointer,
        |installed| prepared.commit(fixture, installed),
    );
    if let Err(error) = completion {
        verify_trace(fixture, recorder, slot.as_ref())?;
        return Err(error);
    }
    slot.ok_or_else(|| "actual initialized state was not installed".into())
}

pub(super) fn advance_and_install(
    fixture: &Fixture,
    recorder: &mut Recorder,
    policy: &mut Policy,
    state: &mut GameState,
    input: GameInput,
    policy_ref: String,
) -> Result<(), String> {
    let (returned, ticket) = deferred::advance_deferred(state, input.clone(), true);
    recorder
        .report
        .native_trace
        .as_mut()
        .ok_or("full native delivery trace missing")?
        .remember_call(input.clone(), returned.clone());
    let candidate = match returned {
        Ok(candidate) => candidate,
        Err(error) => {
            close_without_install(ticket, NonInstallation::CallerAbortedBeforeInstall);
            verify_trace(fixture, recorder, Some(state))?;
            return Err(format!(
                "accepted policy input ({policy_ref}) failed: {error:?}"
            ));
        }
    };
    let prepared = match recorder.prepare_accepted(fixture, state, &input, &candidate, policy_ref) {
        Ok(prepared) => prepared,
        Err(error) => {
            close_without_install(ticket, NonInstallation::RecorderPrepareFailed);
            verify_trace(fixture, recorder, Some(state))?;
            return Err(error);
        }
    };
    let accepted_policy = match policy.prepare_accept(&input, &candidate) {
        Ok(accepted) => accepted,
        Err(error) => {
            drop(prepared);
            close_without_install(ticket, NonInstallation::PolicyAcceptFailed);
            verify_trace(fixture, recorder, Some(state))?;
            return Err(error);
        }
    };
    let pointer = prepared.record_pointer().to_owned();
    let completion = ticket.install_observed(
        candidate,
        GameStateSlot::Advance(state),
        &pointer,
        |installed| {
            prepared.commit(fixture, installed);
            accepted_policy.commit();
        },
    );
    if let Err(error) = completion {
        verify_trace(fixture, recorder, Some(state))?;
        return Err(error);
    }
    Ok(())
}

pub(super) fn probe(
    fixture: &Fixture,
    recorder: &mut Recorder,
    state: &GameState,
    input: GameInput,
) -> Result<Result<NativeCandidate, GameError>, String> {
    let (returned, ticket) = deferred::advance_deferred(state, input.clone(), false);
    recorder
        .report
        .native_trace
        .as_mut()
        .ok_or("full native delivery trace missing")?
        .remember_call(input, returned.clone());
    close_without_install(ticket, NonInstallation::CallerDeclined);
    if returned.is_ok() {
        verify_trace(fixture, recorder, Some(state))?;
    }
    Ok(returned)
}

enum PreparedOutput {
    Initial(Box<Initialization>),
    Accepted(Box<Accepted>),
}
struct CaptureIntent {
    suffix: String,
    owner: &'static str,
}

/// Exclusive preparation capability. It owns only the prospective observer
/// updates, not a clone of the complete report or any new native game state.
/// Dropping it does not change accepted history, rows, journals or capture points.
#[must_use = "commit recorder preparation only after actual state installation"]
pub(super) struct PreparedRecord<'a> {
    recorder: &'a mut Recorder,
    pointer: String,
    history: History,
    world: World,
    selectors: Selectors,
    output: PreparedOutput,
    checkpoint: Option<(usize, FullCheckpoint)>,
    observations: Vec<Observation>,
    journal: RulesJournal,
    captures: Vec<CaptureIntent>,
    trace: Option<TracePosition>,
}
impl PreparedRecord<'_> {
    pub(super) fn record_pointer(&self) -> &str {
        &self.pointer
    }

    /// Publish already validated observer values after the caller has installed
    /// the exact native candidate. Metadata failures remain independently sticky.
    pub(super) fn commit(self, fixture: &Fixture, installed: &GameState) {
        let Self {
            recorder,
            history,
            world,
            selectors,
            output,
            checkpoint,
            observations,
            journal,
            captures,
            trace,
            ..
        } = self;
        // Mark actual installation before any observer allocation/callback can
        // unwind. The exclusive preparation borrow protects these exact indices.
        if let Some(position) = trace {
            let native = recorder
                .report
                .native_trace
                .as_mut()
                .expect("prepared native trace remains present under exclusive borrow");
            match position {
                TracePosition::Initial => native.initial_installed = true,
                TracePosition::Advance { index, ordinal } => {
                    native.calls[index].installed_ordinal = Some(ordinal)
                }
            }
        }
        recorder.history = history;
        recorder.world = Some(world);
        recorder.selectors = selectors;
        match output {
            PreparedOutput::Initial(initial) => recorder.report.initialization = Some(*initial),
            PreparedOutput::Accepted(accepted) => recorder.report.accepted.push(*accepted),
        }
        if let Some((ordinal, checkpoint)) = checkpoint {
            recorder.report.checkpoints.insert(ordinal, checkpoint);
        }
        recorder.report.observations.extend(observations);
        recorder.rules_journals.push(journal);
        for capture in captures {
            if let Err(error) = recorder.capture(fixture, installed, &capture.suffix, capture.owner)
            {
                crate::current_capture::retain_observer_failure(error);
            }
        }
    }
}

struct PreparedNames {
    selectors: Selectors,
    observations: Vec<Observation>,
    checkpoint: Option<(usize, FullCheckpoint)>,
    captures: Vec<CaptureIntent>,
}
impl Recorder {
    fn prepare_names(
        &self,
        fixture: &Fixture,
        source: Option<&GameState>,
        input: Option<&GameInput>,
        candidate: &NativeCandidate,
        ordinal: usize,
        world: &World,
    ) -> Result<PreparedNames, String> {
        let mut selectors = self.selectors.clone();
        let observations = selectors.observe(fixture, source, input, candidate, ordinal)?;
        let mut captures = vec![];
        let checkpoint = if observations
            .iter()
            .any(|name| name.scope == Scope::FullCheckpoint)
        {
            captures.push(CaptureIntent {
                suffix: format!("/checkpoints/{ordinal}"),
                owner: "record::PreparedRecord::commit named checkpoint",
            });
            Some((
                ordinal,
                FullCheckpoint {
                    ordinal,
                    header_ref: ordinal,
                    world: world.clone(),
                },
            ))
        } else {
            None
        };
        let end = self
            .report
            .observations
            .len()
            .checked_add(observations.len())
            .ok_or("prepared observation count exhausted")?;
        for index in self.report.observations.len()..end {
            captures.push(CaptureIntent {
                suffix: format!("/observations/{index}"),
                owner: "record::PreparedRecord::commit named selector",
            });
        }
        Ok(PreparedNames {
            selectors,
            observations,
            checkpoint,
            captures,
        })
    }

    /// Prepare initialization observations through the maintained converters.
    /// No native state or accepted observer history is installed by this method.
    pub(super) fn prepare_initial(
        &mut self,
        fixture: &Fixture,
        candidate: &NativeCandidate,
    ) -> Result<PreparedRecord<'_>, String> {
        if self.report.initialization.is_some() || self.world.is_some() {
            return Err("recorder initialization is already installed".into());
        }
        let trace = if let Some(native) = &self.report.native_trace {
            if native.initial_installed || native.initialization.as_ref().ok() != Some(candidate) {
                return Err("initial preparation differs from retained native Result".into());
            }
            Some(TracePosition::Initial)
        } else {
            None
        };
        let mut history = self.history.clone();
        history.remember(None, candidate);
        let world = view::world(fixture, &history, candidate.state())?;
        let initial = Initialization {
            ordinal: 0,
            base: view::base(fixture, candidate.base())?,
            after_basis: view::basis(fixture, candidate.state().basis())?,
            header: view::header(fixture, &history, candidate.state())?,
            current_facts: view::facts(fixture, &history, None, candidate)?,
            checkpoint_ordinal: 0,
        };
        let names = self.prepare_names(fixture, None, None, candidate, 0, &world)?;
        let pointer = format!("{}/initialization", self.capture_prefix);
        let mut captures = vec![CaptureIntent {
            suffix: "/initialization".into(),
            owner: "record::PreparedRecord::commit initialization",
        }];
        captures.extend(names.captures);
        Ok(PreparedRecord {
            pointer,
            history,
            world,
            selectors: names.selectors,
            output: PreparedOutput::Initial(Box::new(initial)),
            checkpoint: names.checkpoint,
            observations: names.observations,
            journal: candidate.rules_journal().clone(),
            captures,
            trace,
            recorder: self,
        })
    }

    /// Prepare one accepted row, including prospective births/incarnations and
    /// selector state. Any conversion failure discards those prospective values.
    pub(super) fn prepare_accepted(
        &mut self,
        fixture: &Fixture,
        source: &GameState,
        input: &GameInput,
        candidate: &NativeCandidate,
        policy_ref: String,
    ) -> Result<PreparedRecord<'_>, String> {
        let mut history = self.history.clone();
        history.remember(Some(source), candidate);
        let world = view::world(fixture, &history, candidate.state())?;
        let ordinal = self
            .report
            .accepted
            .len()
            .checked_add(1)
            .ok_or("prepared accepted count exhausted")?;
        let trace = if let Some(native) = &self.report.native_trace {
            let index = native
                .calls
                .len()
                .checked_sub(1)
                .ok_or("accepted preparation lacks native call")?;
            let saved = &native.calls[index];
            if saved.input != *input
                || saved.returned.as_ref().ok() != Some(candidate)
                || saved.installed_ordinal.is_some()
            {
                return Err(
                    "accepted preparation differs from retained native input/Result".into(),
                );
            }
            Some(TracePosition::Advance { index, ordinal })
        } else {
            None
        };
        let accepted = accepted_record(
            TransitionObservation {
                fixture,
                history: &history,
                source,
                input,
                candidate,
                before: self.world.as_ref().ok_or("recorder lacks initial world")?,
                after: &world,
            },
            ordinal,
            policy_ref,
        )?;
        let names = self.prepare_names(
            fixture,
            Some(source),
            Some(input),
            candidate,
            ordinal,
            &world,
        )?;
        let suffix = format!("/accepted/{}", self.report.accepted.len());
        let pointer = format!("{}{}", self.capture_prefix, suffix);
        let mut captures = vec![CaptureIntent {
            suffix,
            owner: "record::PreparedRecord::commit accepted",
        }];
        captures.extend(names.captures);
        Ok(PreparedRecord {
            pointer,
            history,
            world,
            selectors: names.selectors,
            output: PreparedOutput::Accepted(Box::new(accepted)),
            checkpoint: names.checkpoint,
            observations: names.observations,
            journal: candidate.rules_journal().clone(),
            captures,
            trace,
            recorder: self,
        })
    }
}

#[cfg(test)]
#[path = "staged/tests.rs"]
mod tests;
