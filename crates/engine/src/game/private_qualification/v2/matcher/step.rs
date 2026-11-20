//! A disclosed prepared Untap followed by the actual scheduler's Upkeep entry.
use super::*;
use crate::turn::{Step, TurnError, TurnStart, WorkCompleted};

#[derive(Debug, PartialEq, Eq)]
pub(in crate::game::private_qualification::v2) enum StepResult {
    Metadata(Box<Result<EffectWindow, EffectError>>),
    Transition(Box<Result<crate::occurrence::RulesTransition<EffectWindow, ()>, EffectError>>),
    Scheduler(Result<TurnScheduler, TurnError>),
}

fn scheduler_view(result: &Result<TurnScheduler, TurnError>) -> Value {
    match result {
        Ok(value) => {
            json!({"tag":"Ok","position":crate::test_support::damage_trace::turn_position_view(value.position()),"stage":format!("{:?}",value.stage()),"lands_played":value.lands_played()})
        }
        Err(error) => projection::turn_error(error),
    }
}

pub(super) fn run(
    source: &EffectWindow,
    active: SeatId,
    turn: u64,
    aliases: &packet::AliasContext,
) -> Result<(Value, MatcherWitness), String> {
    if source.window().turn_owner() != active {
        return Err("matcher step active differs from literal seeded active".into());
    }
    let mut state = source.clone();
    let mut stages = Vec::new();
    let mut records = Vec::new();
    // Each attempt is retained before any adoption. A failed owner call leaves
    // the accepted prefix in final_state; it never manufactures a continuation.
    macro_rules! stop {
        ($actual:expr) => {
            return Ok((json!({"tag":"Err","failed_result":$actual,"preparation_and_entry":records,"state":projection::effect(&state,aliases)?}), MatcherWitness::Step {source:Box::new(source.clone()),stages,final_state:Box::new(state)}))
        };
    }
    let result = crate::test_support::current_capture::turn_new(
        TurnStart {
            number: turn,
            step: Step::Untap,
        },
        source.window().participants().len() == 2,
    );
    let view = scheduler_view(&result);
    records.push(json!({"operation":"TurnScheduler.new(Untap)","role":"explicit-prepared-calendar","result":view}));
    stages.push(StepResult::Scheduler(result.clone()));
    let old = match result {
        Ok(value) => value,
        Err(_) => stop!(view),
    };

    let expected = state.revision();
    let result = capture::transition(
        &state,
        aliases,
        projection::EffectCall::EnterTurnStep {
            expected,
            active,
            priority: false,
            empty_mana: false,
        },
        |source| source.enter_turn_step(expected, active, false, false),
        |_| Value::Null,
        true,
    )?;
    let view = transition_view(&result, aliases)?;
    records.push(json!({"operation":"EffectWindow.enter_turn_step","role":"prepared-Untap-metadata-no-retrospective-StepBegan","result":view}));
    stages.push(StepResult::Transition(Box::new(result.clone())));
    state = match result {
        Ok(value) => value.into_state(),
        Err(_) => stop!(view),
    };

    let expected = state.revision();
    let result = capture::state(
        &state,
        aliases,
        projection::EffectCall::RecordTurnStart { expected },
        |source| source.record_turn_start(expected),
        true,
    )?;
    let view = match &result {
        Ok(value) => json!({"tag":"Ok","state":projection::effect(value,aliases)?}),
        Err(error) => projection::effect_error(error),
    };
    records.push(json!({"operation":"EffectWindow.record_turn_start","role":"prepared-control-history-metadata-no-append","result":view}));
    stages.push(StepResult::Metadata(Box::new(result.clone())));
    state = match result {
        Ok(value) => value,
        Err(_) => stop!(view),
    };

    let expected = state.revision();
    let result = capture::transition(
        &state,
        aliases,
        projection::EffectCall::UntapOnTurn {
            expected,
            turn: &old,
        },
        |source| source.untap_on_turn(expected, &old),
        |_| Value::Null,
        true,
    )?;
    let view = transition_view(&result, aliases)?;
    records.push(
        json!({"operation":"EffectWindow.untap_on_turn","role":"actual-Untap-work","result":view}),
    );
    stages.push(StepResult::Transition(Box::new(result.clone())));
    state = match result {
        Ok(value) => value.into_state(),
        Err(_) => stop!(view),
    };

    let result = crate::test_support::current_capture::turn_work(&old, WorkCompleted::Untap);
    let view = scheduler_view(&result);
    records.push(json!({"operation":"TurnScheduler.complete_work(Untap)","result":view}));
    stages.push(StepResult::Scheduler(result.clone()));
    let next = match result {
        Ok(value) => value,
        Err(_) => stop!(view),
    };

    let expected = state.revision();
    let result = capture::transition(
        &state,
        aliases,
        projection::EffectCall::EnterCalendar {
            expected,
            previous: Some(&old),
            next_turn: &next,
            active,
        },
        |source| source.enter_calendar(expected, Some(&old), &next, active),
        |_| Value::Null,
        true,
    )?;
    let view = transition_view(&result, aliases)?;
    records.push(json!({"operation":"EffectWindow.enter_calendar","role":"genuine-Untap-to-Upkeep-entry","result":view}));
    stages.push(StepResult::Transition(Box::new(result.clone())));
    state = match result {
        Ok(value) => value.into_state(),
        Err(_) => stop!(view),
    };
    Ok((
        json!({"tag":"Ok","preparation_and_entry":records,"state":projection::effect(&state,aliases)?,"root_or_reference_trajectory_claim":false}),
        MatcherWitness::Step {
            source: Box::new(source.clone()),
            stages,
            final_state: Box::new(state),
        },
    ))
}

fn transition_view(
    result: &Result<crate::occurrence::RulesTransition<EffectWindow, ()>, EffectError>,
    aliases: &packet::AliasContext,
) -> Result<Value, String> {
    Ok(match result {
        Ok(value) => {
            json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"append":projection::journal(value.append())})
        }
        Err(error) => projection::effect_error(error),
    })
}
