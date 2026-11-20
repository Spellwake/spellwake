//! Trusted precheck/cohort units using actual EffectWindow owner adapters.
use super::*;

pub(super) fn run(
    selected: &packet::SelectedInput,
    case: &str,
) -> Result<(Value, Witness), String> {
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    let aliases = prepared.aliases;
    let mut state = match crate::test_support::native_calls::effect_initialize(prepared.seed) {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","scope":selected.fixture["scope"],"initialization":projection::effect_error(&error),"state":null,"stop_reached":false}),
                Witness::EffectInitialization(Box::new(Err(error))),
            ));
        }
    };
    let seeded = state.clone();
    let joins = Joins::seed(&selected.fixture, &state)?;
    let actions = rows(&selected.fixture, "actions")?;
    let [action] = actions else {
        return Err("trusted foundation requires one literal call".into());
    };
    match case {
        "T13" => {
            if string(action, "op")? != "trusted_owned_batch_entry" {
                return Err("wrong T13 owner operation".into());
            }
            let moves = rows(action, "moves")?
                .iter()
                .map(|row| {
                    Ok((
                        joins.object(&row["card"], &selected.fixture, &state)?,
                        ObjectDestination::Battlefield {
                            controller: packet::seat(string(&row["destination"], "Battlefield")?)?,
                        },
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let expected = state.revision();
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
            let actual = match &result {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),&aliases)?,"movements":value.result().iter().map(|movement|json!({"before":projection::object(&movement.before),"after":projection::object(&movement.after)})).collect::<Vec<_>>(),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            };
            Ok((
                capture::effect_record_point(
                    json!({"status":if result.is_ok(){"completed_recipe"}else{"failed"},"scope":selected.fixture["scope"],"seeded":projection::effect(&seeded,&aliases)?,"result":actual,"source_full_state_eq":state==seeded,"stop":selected.fixture["stop"]}),
                    &seeded,
                    &aliases,
                    "trusted::run actual selected moves",
                    "/observation/seeded",
                )?,
                Witness::Moves {
                    source: Box::new(seeded),
                    result: Box::new(result),
                },
            ))
        }
        "T14" => {
            for row in &prepared.trusted_precheck_overrides {
                if string(row, "field")? != "physical_pt_counters" {
                    return Err("unsupported precheck preparation".into());
                }
                let object = joins.object(&row["object"], &selected.fixture, &state)?;
                let source = state.clone();
                let counters = vec![(
                    object,
                    PowerToughnessCounters {
                        plus_one: quantity(&row["plus_one"])?,
                        minus_one: quantity(&row["minus_one"])?,
                    },
                )];
                let result = capture::state(
                    &state,
                    &aliases,
                    projection::EffectCall::QualificationPrepareCreatureFields {
                        counters: &counters,
                        marked: &[],
                    },
                    |source| source.qualification_prepare_creature_fields(&counters, &[]),
                    true,
                )?;
                if let Ok(next) = &result {
                    state = next.clone();
                }
                if let Err(error) = result {
                    return Ok((
                        json!({"status":"failed","scope":selected.fixture["scope"],"seeded":projection::effect(&seeded,&aliases)?,"preparation":{"input":row,"source":projection::effect(&source,&aliases)?,"result":projection::effect_error(&error),"retained_source_full_state_eq":state==source},"retained_state":projection::effect(&state,&aliases)?,"stop_reached":false}),
                        Witness::EffectPreparation {
                            seeded: Box::new(seeded),
                            source: Box::new(source),
                            result: Err(error),
                            retained: Box::new(state),
                        },
                    ));
                }
            }
            if string(action, "op")? != "trusted_call"
                || string(action, "owner")? != "EffectWindow"
                || string(action, "method")? != "begin_checks"
            {
                return Err("wrong T14 owner operation".into());
            }
            let prepared = state.clone();
            let expected = state.revision();
            let result = capture::progress(
                &state,
                &aliases,
                projection::EffectCall::BeginChecks { expected },
                |source| source.begin_checks(expected),
                true,
            )?;
            let actual = match &result {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),&aliases)?,"typed_result":format!("{:?}",value.result()),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            };
            Ok((
                capture::effect_record_point(
                    json!({"status":if result.is_ok(){"completed_recipe"}else{"failed"},"scope":selected.fixture["scope"],"seeded":projection::effect(&seeded,&aliases)?,"prepared":projection::effect(&prepared,&aliases)?,"result":actual,"source_full_state_eq":state==prepared,"stop":selected.fixture["stop"]}),
                    &prepared,
                    &aliases,
                    "trusted::run actual selected checks",
                    "/observation/prepared",
                )?,
                Witness::Checks {
                    seeded: Box::new(seeded),
                    prepared: Box::new(prepared),
                    result: Box::new(result),
                },
            ))
        }
        _ => Err("unsupported trusted qualification case".into()),
    }
}
