//! The matcher profile invokes actual mutation owners and their single publisher.
use super::*;
use crate::object_identity::ObjectStatusChange;

mod input;
mod step;
#[cfg(test)]
mod tests;
use input::{Case, Operation};

pub(super) const INPUT_SHA256: &str =
    "80934757c10afcda10824f4c54da53717783b3f5e29128c13615595e715fd94a";

pub(super) fn preflight(
    input: &Value,
    definitions: &BTreeMap<String, Value>,
) -> Result<Vec<String>, String> {
    if input["schema"] != "spellwake.ordinary-trigger-matcher-inputs.v9"
        || input["schema_version"] != 9
    {
        return Err("wrong matcher matrix schema".into());
    }
    let vocabulary = std::sync::Arc::new(creature_types::vocabulary(
        input["creature_vocabulary"].clone(),
    )?);
    rows(input, "cases")?
        .iter()
        .map(|raw| {
            let case: Case =
                serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
            input::prepare(&case, &input["shared_seed"], definitions, &vocabulary)?;
            Ok(case.id)
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum MatcherWitness {
    Initialization(Box<Result<EffectWindow, EffectError>>),
    Moves {
        source: Box<EffectWindow>,
        result: Box<
            Result<
                crate::occurrence::RulesTransition<EffectWindow, Vec<ObjectMovement>>,
                EffectError,
            >,
        >,
    },
    Status {
        source: Box<EffectWindow>,
        result: Box<
            Result<
                crate::occurrence::RulesTransition<EffectWindow, Vec<ObjectStatusChange>>,
                EffectError,
            >,
        >,
    },
    Step {
        source: Box<EffectWindow>,
        stages: Vec<step::StepResult>,
        final_state: Box<EffectWindow>,
    },
}

fn zone(value: ObjectZone) -> &'static str {
    match value {
        ObjectZone::Hand(_) => "Hand",
        ObjectZone::Battlefield => "Battlefield",
        ObjectZone::Graveyard(_) => "Graveyard",
        ObjectZone::Exile => "Exile",
        ObjectZone::Command => "Command",
        ObjectZone::Library(_) => "Library",
        ObjectZone::Stack => "Stack",
    }
}

pub(super) fn run(
    input: &Value,
    case_id: &str,
    definitions: &BTreeMap<String, Value>,
) -> Result<(Value, V2Witness), String> {
    if input["schema"] != "spellwake.ordinary-trigger-matcher-inputs.v9"
        || input["schema_version"] != 9
    {
        return Err("unsupported matcher matrix schema".into());
    }
    let cases = rows(input, "cases")?;
    let mut ids = BTreeSet::new();
    for row in cases {
        if !ids.insert(string(row, "id")?) {
            return Err("duplicate matcher case ID".into());
        }
    }
    let raw = cases
        .iter()
        .find(|value| value["id"] == case_id)
        .ok_or("matcher case not found")?;
    let case: Case = serde_json::from_value(raw.clone()).map_err(|error| error.to_string())?;
    if case.id != case_id
        || !case.capture_seed_handles
        || case.stop
            != "after accepted single publisher transition before checks/placement/body execution"
    {
        return Err("unsupported matcher capture/stop".into());
    }
    let vocabulary = std::sync::Arc::new(creature_types::vocabulary(
        input["creature_vocabulary"].clone(),
    )?);
    let (seed, aliases, physical) =
        input::prepare(&case, &input["shared_seed"], definitions, &vocabulary)?;
    let state = match crate::test_support::native_calls::effect_initialize(seed) {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","scope":case.scope,"initialization":projection::effect_error(&error),"source_state":null}),
                V2Witness::Matcher(Box::new(MatcherWitness::Initialization(Box::new(Err(
                    error,
                ))))),
            ));
        }
    };
    let handles = physical
        .iter()
        .map(|(alias, card)| {
            let id = state
                .window()
                .cards()
                .current_identity(*card)
                .ok_or("matcher seed handle missing")?;
            Ok((alias.clone(), id))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let source_view = projection::effect(&state, &aliases)?;
    let seed_handles=json!(handles.iter().map(|(alias,id)|Ok(json!({"alias":alias,"physical":projection::object(state.window().cards().get(*id).ok_or("seed object missing")?)}))).collect::<Result<Vec<_>,String>>()?);
    let object = |alias: &str| {
        handles
            .get(alias)
            .copied()
            .ok_or_else(|| format!("unbound exact seed object {alias}"))
    };
    let (actual, witness) = match &case.operation {
        Operation::Zone {
            object_alias,
            from,
            to,
            new_controller,
        } => {
            let object = object(object_alias)?;
            if zone(
                state
                    .window()
                    .cards()
                    .get(object)
                    .ok_or("matcher object missing")?
                    .zone(),
            ) != from
            {
                return Err("declared matcher origin differs from actual object".into());
            }
            let destination = input::destination(to, new_controller.as_deref())?;
            let expected = state.revision();
            let moves = vec![(object, destination)];
            let result = capture::transition(
                &state,
                &aliases,
                projection::EffectCall::QualificationZoneMoves {
                    expected,
                    moves: &moves,
                },
                |source| source.qualification_zone_moves(expected, &moves),
                |movements| {
                    json!(movements.iter().map(|movement|json!({"before":projection::object(&movement.before),"after":projection::object(&movement.after)})).collect::<Vec<_>>())
                },
                true,
            )?;
            let actual = move_view(&result, &aliases)?;
            (
                actual,
                MatcherWitness::Moves {
                    source: Box::new(state.clone()),
                    result: Box::new(result),
                },
            )
        }
        Operation::Discard {
            discarding_player,
            discarded_card,
        } => {
            let expected = state.revision();
            let player = packet::seat(discarding_player)?;
            let object = object(&discarded_card.alias)?;
            let result = capture::transition(
                &state,
                &aliases,
                projection::EffectCall::QualificationDiscard {
                    expected,
                    player,
                    object,
                },
                |source| source.qualification_discard(expected, player, object),
                |movements| {
                    json!(movements.iter().map(|movement|json!({"before":projection::object(&movement.before),"after":projection::object(&movement.after)})).collect::<Vec<_>>())
                },
                true,
            )?;
            let actual = move_view(&result, &aliases)?;
            (
                actual,
                MatcherWitness::Moves {
                    source: Box::new(state.clone()),
                    result: Box::new(result),
                },
            )
        }
        Operation::Tap {
            object_alias,
            before_tapped,
            after_tapped,
        } => {
            let object = object(object_alias)?;
            if state
                .window()
                .cards()
                .get(object)
                .and_then(|card| card.tapped())
                != Some(*before_tapped)
            {
                return Err("matcher before tap value differs from actual source".into());
            }
            let expected = state.revision();
            let objects = [object];
            let tapped = *after_tapped;
            let result = capture::transition(
                &state,
                &aliases,
                projection::EffectCall::QualificationSetTapped {
                    expected,
                    objects: &objects,
                    tapped,
                },
                |source| source.qualification_set_tapped(expected, &objects, tapped),
                |changes| {
                    json!(changes.iter().map(|change|json!({"before":projection::object(&change.before),"after":projection::object(&change.after)})).collect::<Vec<_>>())
                },
                true,
            )?;
            let actual = match &result {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),&aliases)?,"changes":value.result().iter().map(|pair|json!({"before":projection::object(&pair.before),"after":projection::object(&pair.after)})).collect::<Vec<_>>(),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            };
            (
                actual,
                MatcherWitness::Status {
                    source: Box::new(state.clone()),
                    result: Box::new(result),
                },
            )
        }
        Operation::Step { active, turn } => {
            step::run(&state, packet::seat(active)?, *turn, &aliases)?
        }
    };
    let status = if actual["tag"] == "Ok" {
        "completed_recipe"
    } else {
        "failed"
    };
    Ok((
        capture::effect_record_point(
            json!({"status":status,"scope":case.scope,"selected_input":raw,"shared_seed":input["shared_seed"],"source_state":source_view,"seed_handles":seed_handles,"operation_result":actual,"stop":case.stop,"checks_placement_or_body_execution":false}),
            &state,
            &aliases,
            "matcher::run actual selected owner",
            "/observation/source_state",
        )?,
        V2Witness::Matcher(Box::new(witness)),
    ))
}

fn move_view(
    result: &Result<
        crate::occurrence::RulesTransition<EffectWindow, Vec<ObjectMovement>>,
        EffectError,
    >,
    aliases: &packet::AliasContext,
) -> Result<Value, String> {
    Ok(match result {
        Ok(value) => {
            json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"movements":value.result().iter().map(|pair|json!({"before":projection::object(&pair.before),"after":projection::object(&pair.after)})).collect::<Vec<_>>(),"append":projection::journal(value.append())})
        }
        Err(error) => projection::effect_error(error),
    })
}
