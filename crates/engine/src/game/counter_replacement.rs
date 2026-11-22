//! Resume the exact native action whose tentative cut needs a counter order.
use super::*;
use crate::effects::CounterReplacementChoice;

/// The accepted executor stays at the pre-action cut until all placement choices
/// have been provided. Deterministic entropy and normal action validation are
/// replayed from that same executor with the already issued answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SuspendedCounterPlacement {
    input: GameInput,
    previous_decision: Option<DecisionState>,
    choices: Vec<(CounterReplacementChoice, usize)>,
}

impl GameState {
    pub(super) fn advance_with_counter_replacement(
        &self,
        input: GameInput,
    ) -> Result<NativeCandidate, GameError> {
        self.validate_basis(&input.basis)?;
        if !self
            .execution
            .window()
            .participants()
            .contains(&input.actor)
        {
            return Err(GameError::UnknownActor);
        }
        if self.outcome().is_some() {
            return Err(GameError::GameEnded);
        }
        if matches!(input.command, GameCommand::Concede) {
            return self.advance_inner(input);
        }
        let mut work;
        let result = if let Some(suspended) = &self.counter_replacement {
            let pending = self
                .pending
                .as_ref()
                .ok_or(GameError::UnexpectedKernelPhase)?;
            let RuleFrame::CounterReplacement(frame) = &pending.frame else {
                return Err(GameError::UnexpectedKernelPhase);
            };
            let GameCommand::Reply { decision, choice } = input.command else {
                return Err(GameError::DecisionRequired);
            };
            if decision != pending.id {
                return Err(GameError::WrongDecision);
            }
            if input.actor != pending.chooser {
                return Err(GameError::WrongChooser);
            }
            let DecisionReply::ApplyCounterDoublersFirst(number) = choice else {
                return Err(EffectError::WrongDecisionKind.into());
            };
            if number > frame.doublers() {
                return Err(EffectError::IllegalTarget.into());
            }
            work = suspended.clone();
            work.choices.push((frame.clone(), number));
            let mut replay = self.clone();
            replay.pending = work.previous_decision.clone();
            replay.counter_replacement = None;
            replay
                .execution
                .prepare_counter_replacements(work.choices.clone());
            let mut original = work.input.clone();
            original.basis = self.basis.clone();
            replay.advance_inner(original)
        } else {
            work = SuspendedCounterPlacement {
                input: input.clone(),
                previous_decision: self.pending.clone(),
                choices: Vec::new(),
            };
            self.advance_inner(input)
        };
        match result {
            Ok(mut candidate) => {
                candidate.next.execution.finish_counter_replacements()?;
                Ok(candidate)
            }
            Err(GameError::Effect(EffectError::CounterReplacementRequired(choice))) => {
                let mut next = self.clone();
                next.basis.revision = self
                    .basis
                    .revision
                    .checked_add(1)
                    .ok_or(GameError::RevisionExhausted)?;
                next.bind_decision(RuleFrame::CounterReplacement(choice))?;
                next.counter_replacement = Some(work);
                Ok(NativeCandidate {
                    base: CandidateBase::Advance(self.basis.clone()),
                    next,
                    facts: TransitionFacts::default(),
                })
            }
            Err(error) => Err(error),
        }
    }
}
