//! Source-bound qualification authorities. These are actual native values, not a durable host acceptance proof.
use super::probes::{ConsumedPlan, PendingObligation, ProbeAuthorityReceipt};
use super::{SelectionSource, probes::ReachedSelection};
use crate::ordinary_game::{
    fixture::{CaseId, Fixture, FixtureInitialization},
    policy::Policy,
    selection::data::SelectionCase,
};
use spellwake_engine::game::{
    CandidateBase, DecisionReply, DecisionState, GameBasis, GameCommand, GameError, GameInput,
    GameState, NativeCandidate, RuleFrame,
};
use std::{collections::BTreeMap, sync::Arc};

/// An actual helper call, retained independently of the main accepted stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignHelperCall {
    pub source: GameState,
    pub input: GameInput,
    pub result: Result<NativeCandidate, GameError>,
    pub installed: bool,
}
/// The bounded genuine SD11 prefix stops before its owner-order reply. No full SD11 credit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignHelperTrace {
    pub initialization: Result<NativeCandidate, GameError>,
    pub validation: Option<Result<NativeCandidate, GameError>>,
    pub calls: Vec<ForeignHelperCall>,
    pub stop: Option<GameState>,
    pub(in crate::ordinary_game) stop_binding: Option<Arc<ReachedSelection>>,
    pub(in crate::ordinary_game) consumed_plans: BTreeMap<String, ConsumedPlan>,
}
/// Separate actual installed initializer and unadopted source-fixture recomputation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionBindingEvidence {
    pub installed_initializer: NativeCandidate,
    pub validation: Result<NativeCandidate, GameError>,
    pub foreign_helper: Option<ForeignHelperTrace>,
}
/// Failed binding retains any actual auxiliary native result and helper prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionBindingError {
    pub message: String,
    pub validation: Option<Box<Result<NativeCandidate, GameError>>>,
    pub helper: Option<Box<ForeignHelperTrace>>,
}
impl SelectionBindingError {
    pub(in crate::ordinary_game) fn caller(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            validation: None,
            helper: None,
        }
    }
    /// Retain the actual outer initializer result when later helper preparation fails.
    /// The helper owner retains its own validation inside the separate helper trace.
    pub(in crate::ordinary_game) fn with_main_validation(
        mut self,
        validation: Result<NativeCandidate, GameError>,
    ) -> Self {
        self.validation = Some(Box::new(validation));
        self
    }
    pub(in crate::ordinary_game) fn validation(
        message: impl Into<String>,
        result: Result<NativeCandidate, GameError>,
    ) -> Self {
        Self {
            message: message.into(),
            validation: Some(Box::new(result)),
            helper: None,
        }
    }
}
impl std::fmt::Display for SelectionBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for SelectionBindingError {}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct ForeignAuthority {
    state: GameState,
    pub trace: ForeignHelperTrace,
    binding: Arc<ReachedSelection>,
}
impl ForeignAuthority {
    pub(in crate::ordinary_game) fn basis(&self) -> &GameBasis {
        self.state.basis()
    }
    pub(in crate::ordinary_game) fn decision(&self) -> &DecisionState {
        &self.binding.decision
    }
    pub(in crate::ordinary_game) fn binding(&self) -> &Arc<ReachedSelection> {
        &self.binding
    }
    pub(in crate::ordinary_game) fn validate_stop(
        policy: &Policy,
        state: &GameState,
    ) -> Result<Arc<ReachedSelection>, String> {
        let prepared = policy
            .prepare(state)?
            .ok_or("foreign helper stopped without a required order plan")?;
        let metadata = prepared
            .selection_metadata()
            .ok_or("foreign helper stop lacks matched selected evidence")?;
        if !metadata.receipts.is_empty() || !metadata.pending.is_empty() {
            return Err("foreign helper stop unexpectedly carries probes".into());
        }
        if metadata.reached.case != SelectionCase::Sd11
            || !matches!(
                metadata.reached.decision.frame(),
                RuleFrame::ResolutionGraveyardOrder(_)
            )
            || metadata.reached.decision.basis() != state.basis()
        {
            return Err("foreign helper stopped at an unexpected matched frame".into());
        }
        if policy
            .selection_observation()
            .ok_or("foreign helper selected annotations missing")?
            .consumed
            .contains_key(&metadata.reached.plan_id)
        {
            return Err("foreign helper order plan was already consumed".into());
        }
        Ok(metadata.reached.clone())
    }
    pub(in crate::ordinary_game) fn prepare(
        source: &SelectionSource,
    ) -> Result<Self, SelectionBindingError> {
        Self::prepare_with_stop_annotation(source, |_| Ok(()))
    }
    pub(in crate::ordinary_game) fn prepare_with_stop_annotation(
        source: &SelectionSource,
        inspect: impl FnOnce(&mut Policy) -> Result<(), String>,
    ) -> Result<Self, SelectionBindingError> {
        let fixture = source
            .fixture(SelectionCase::Sd11, source.vocabulary().clone())
            .map_err(SelectionBindingError::caller)?;
        let initialized = initialize(&fixture);
        let mut trace = ForeignHelperTrace {
            initialization: initialized.clone(),
            validation: None,
            calls: vec![],
            stop: None,
            stop_binding: None,
            consumed_plans: BTreeMap::new(),
        };
        let run = (|| -> Result<(GameState, Arc<ReachedSelection>), String> {
            let candidate =
                initialized.map_err(|error| format!("foreign helper initializer: {error:?}"))?;
            let mut state = candidate.state().clone();
            let mut policy = Policy::new(&fixture)?;
            let evidence = match policy.bind_selection_initializer(source, &state, &candidate) {
                Ok(evidence) => evidence,
                Err(error) => {
                    let message = error.to_string();
                    trace.validation = error.validation.map(|result| *result);
                    return Err(message);
                }
            };
            if evidence.foreign_helper.is_some() {
                return Err("foreign helper cannot recursively provision foreign authority".into());
            }
            trace.validation = Some(evidence.validation);
            for index in 0..4 {
                let prepared = policy
                    .prepare(&state)?
                    .ok_or("foreign helper stopped before its four-input prefix")?;
                let input = policy.issue(&state, prepared)?;
                let shape = matches!(
                    (index, &input.command),
                    (0, GameCommand::CastPaid { .. })
                        | (1 | 2, GameCommand::Pass)
                        | (
                            3,
                            GameCommand::Reply {
                                choice: DecisionReply::SelectResolution(_),
                                ..
                            }
                        )
                );
                if !shape {
                    return Err(
                        "foreign helper shared-policy input differs from its fixed source prefix"
                            .into(),
                    );
                }
                let result = state.advance(input.clone());
                trace.calls.push(ForeignHelperCall {
                    source: state.clone(),
                    input: input.clone(),
                    result: result.clone(),
                    installed: false,
                });
                let candidate =
                    result.map_err(|error| format!("foreign helper native input: {error:?}"))?;
                let acceptance = policy.prepare_accept(&input, &candidate)?;
                state = candidate.into_state();
                acceptance.commit();
                trace.consumed_plans = policy
                    .selection_observation()
                    .ok_or("foreign helper selected progress absent")?
                    .consumed;
                trace
                    .calls
                    .last_mut()
                    .ok_or("foreign helper trace append missing")?
                    .installed = true;
            }
            trace.stop = Some(state.clone());
            inspect(&mut policy)?;
            let binding = Self::validate_stop(&policy, &state)?;
            trace.stop_binding = Some(binding.clone());
            Ok((state, binding))
        })();
        match run {
            Ok((state, binding)) => {
                trace.stop = Some(state.clone());
                Ok(Self {
                    state,
                    trace,
                    binding,
                })
            }
            Err(message) => Err(SelectionBindingError {
                message,
                validation: None,
                helper: Some(Box::new(trace)),
            }),
        }
    }
}
pub(in crate::ordinary_game) fn initialize(
    fixture: &Fixture,
) -> Result<NativeCandidate, GameError> {
    match fixture.initialization {
        FixtureInitialization::Scheduled(start) => GameState::initialize_scheduled(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.seed.clone(),
            start,
        ),
        FixtureInitialization::Opening => GameState::initialize_opening(
            fixture.game,
            fixture.context.clone(),
            fixture.setup.clone(),
        ),
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct InitializerAuthority {
    pub basis: GameBasis,
    pub evidence: SelectionBindingEvidence,
    pub foreign: Option<ForeignAuthority>,
}
/// Privileged controller annotations for future qualification consumers, not Report5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::ordinary_game) struct SelectionProgressObservation {
    pub consumed: BTreeMap<String, ConsumedPlan>,
    pub receipts: Vec<ProbeAuthorityReceipt>,
    pub pending: Vec<PendingObligation>,
}
/// Completely bind the policy's exact fixture to the actual source and full native result.
pub(in crate::ordinary_game) fn validate_initializer(
    source: &SelectionSource,
    fixture: &Fixture,
    installed: &GameState,
    initializer: &NativeCandidate,
) -> Result<Result<NativeCandidate, GameError>, SelectionBindingError> {
    let CaseId::Selection(case) = fixture.case else {
        return Err(SelectionBindingError::caller(
            "initializer binding requires selected fixture",
        ));
    };
    let owned = source
        .fixture(case, source.vocabulary().clone())
        .map_err(SelectionBindingError::caller)?;
    if fixture != &owned {
        return Err(SelectionBindingError::caller(
            "selected policy Fixture differs from the complete source-owned fixture",
        ));
    }
    if installed != initializer.state()
        || initializer.base()
            != &(CandidateBase::Initialize {
                game: owned.game,
                context: owned.context.clone(),
            })
    {
        return Err(SelectionBindingError::caller(
            "selected initializer differs from the supplied installed native state/base",
        ));
    }
    let validation = initialize(&owned);
    if validation.as_ref().ok() != Some(initializer) {
        return Err(SelectionBindingError::validation(
            "selected initializer differs from the complete genuine source-fixture recomputation",
            validation,
        ));
    }
    Ok(validation)
}
