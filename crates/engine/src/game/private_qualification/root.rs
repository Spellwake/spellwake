//! Finite caller of the actual GameState owner, with disclosed cloned-source faults.
use super::*;
mod extended;
pub(super) use extended::FrameAttempt;

pub(super) fn fault(
    state: &mut GameState,
    owner: &str,
    field: &str,
    value: u64,
) -> Result<(), String> {
    let effect = match (owner, field) {
        ("GameState", "next_decision") => {
            state.next_decision = value;
            return Ok(());
        }
        ("GameState", "basis.revision") => {
            state.basis.revision = value;
            return Ok(());
        }
        ("EffectWindow", "revision") => QualificationFault::Revision(value),
        ("StackWindow", "revision") => QualificationFault::WindowRevision(value),
        ("PriorityRound", "revision") => QualificationFault::PriorityRevision(value),
        ("GameObjects", "revision") => QualificationFault::ObjectRevision(value),
        ("GameObjects", "next_identity") => QualificationFault::ObjectIdentity(value),
        ("StackWindow", "next_entry") => QualificationFault::StackEntry(value),
        ("TriggerState", "next_event") => QualificationFault::Event(value),
        ("TriggerState", "next_instance") => QualificationFault::Instance(value),
        ("TriggerState", "next_wave") => QualificationFault::Wave(value),
        ("CharacteristicState", "next_effect") => QualificationFault::ContinuousEffect(value),
        ("CharacteristicState", "next_timestamp") => {
            QualificationFault::CharacteristicTimestamp(value)
        }
        ("CharacteristicState", "next_world_tenure") => QualificationFault::WorldTenure(value),
        ("DamageState", "next_shield") => QualificationFault::Shield(value),
        ("DamageState", "next_multiplier") => QualificationFault::Multiplier(value),
        ("DamageState", "next_batch") => QualificationFault::DamageBatch(value),
        ("DamageState", "next_attempt") => QualificationFault::DamageAttempt(value),
        _ => return Err(format!("unsupported private fault {owner}.{field}")),
    };
    state.execution.qualification_set_fault(effect);
    Ok(())
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, Witness), String> {
    run_profile(selected, false).map(|(report, witness, _)| (report, witness))
}
pub(super) fn run_extended(
    selected: &packet::SelectedInput,
) -> Result<(Value, Witness, Vec<FrameAttempt>), String> {
    run_profile(selected, true)
}
fn run_profile(
    selected: &packet::SelectedInput,
    extended: bool,
) -> Result<(Value, Witness, Vec<FrameAttempt>), String> {
    let prepared = selected.prepare_seed(
        if extended
            && selected.fixture["seed"]["initial_boundary"]["kind"]
                == "ScheduledPrivatePreparedResourceUnit"
        {
            packet::SeedMode::TrustedUnit
        } else {
            packet::SeedMode::PreparedRoot
        },
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if prepared.initial_status.iter().any(|(_, tapped)| *tapped)
        || !prepared.trusted_precheck_overrides.is_empty()
    {
        return Err("unsupported private root initial preparation".into());
    }
    let initialization = match prepared.initial {
        packet::Initial::Scheduled { turn, step }
        | packet::Initial::ScheduledPrivatePreparedResourceUnit { turn, step } => {
            crate::test_support::current_capture::initialize_scheduled(
                prepared.game,
                prepared.context,
                prepared.seed,
                TurnStart {
                    number: turn,
                    step: program::step(&step)?,
                },
            )
        }
        packet::Initial::Interaction {} => crate::test_support::current_capture::initialize(
            prepared.game,
            prepared.context,
            prepared.seed,
        ),
        packet::Initial::Opening {
            hand_size,
            entropy_words,
        } => {
            if hand_size != HAND_SIZE {
                return Err("unsupported opening hand size".into());
            }
            crate::test_support::current_capture::initialize_opening(
                prepared.game,
                prepared.context,
                OpeningSetup {
                    seed: prepared.seed,
                    entropy: EntropyTape::new(entropy_words.clone()),
                },
                entropy_words,
            )
        }
        packet::Initial::TrustedUnitPrecheck {} => {
            return Err("trusted precheck is not a root initializer".into());
        }
    };
    let initialization = match initialization {
        Ok(value) => value,
        Err(error) => {
            let report = json!({"status":"failed","scope":selected.fixture["scope"],"records":[{"op":"initialize","result":projection::game_error(&error)}],"failure":"actual root initialization rejected","final_state":null,"completed_action_ordinal":0,"stop_reached":false});
            return Ok((
                report,
                Witness::Root {
                    initialization: Box::new(Err(error)),
                    attempts: vec![],
                    final_state: None,
                },
                vec![],
            ));
        }
    };
    let aliases = prepared.aliases;
    let mut state = initialization.state().clone();
    let mut joins = if extended {
        Joins::seed_from_candidate(&selected.fixture, &initialization)?
    } else {
        Joins::seed(&selected.fixture, &state.execution)?
    };
    joins.observe(&selected.fixture, initialization.rules_journal())?;
    let mut records = vec![capture::root_point(
        json!({"op":"initialize","candidate":projection::candidate(&initialization,None,&aliases)?}),
        initialization.state(),
        &aliases,
        "root::run actual initialization",
        "/observation/records/0/candidate/state",
    )?];
    let mut snapshots = BTreeMap::<String, GameState>::new();
    let mut attempts = Vec::new();
    let mut frame_attempts = Vec::new();
    let actions = rows(&selected.fixture, "actions")?;
    if selected.fixture["stop"]["after_action_ordinal"].as_u64() != Some(actions.len() as u64)
        || selected.fixture["stop"]["no_extra_pass"] != true
    {
        return Err("private recipe has no exact finite stop".into());
    }
    let mut failure = None;
    let mut failed_ordinal = None;
    let mut completed_ordinal = 0;
    let calls = if extended {
        extended::calls(actions)?
    } else {
        actions
            .iter()
            .enumerate()
            .map(|(index, value)| (index + 1, value))
            .collect()
    };
    'actions: for (ordinal, action) in calls {
        let op = string(action, "op")?;
        if extended && let Err(error) = extended::opening_guard(&state, action) {
            failure = Some(error);
            failed_ordinal = Some(ordinal);
            break;
        }
        match op {
            "private_fault_clone" => {
                let mut clone = if extended {
                    action["snapshot"]
                        .as_str()
                        .map(|name| {
                            snapshots
                                .get(name)
                                .cloned()
                                .ok_or("unknown private clone source")
                        })
                        .transpose()?
                        .unwrap_or_else(|| state.clone())
                } else {
                    state.clone()
                };
                let owner = string(action, "owner")?;
                let field = string(action, "field")?;
                let value = integer(&action["value"])?;
                fault(&mut clone, owner, field, value)?;
                let name = string(action, "bind")?;
                records.push(capture::root_point(json!({"ordinal":ordinal,"op":op,"source":projection::state(&state,&aliases)?,"owner":owner,"field":field,"value":value.to_string(),"binding":name,"fault_source":projection::state(&clone,&aliases)?,"scope":"private_clone_not_GameInput"}),&clone,&aliases,"root::run actual scoped record",&format!("/observation/records/{}/fault_source",records.len()))?);
                snapshots.insert(name.into(), clone);
            }
            "adopt_private_fault_clone_as_unit_source" => {
                let name = string(action, "snapshot")?;
                state = snapshots.get(name).ok_or("unknown private source")?.clone();
                records.push(capture::root_point(json!({"ordinal":ordinal,"op":op,"snapshot":name,"state":projection::state(&state,&aliases)?,"scope":"explicit_private_unit_source_not_player_command"}),&state,&aliases,"root::run actual scoped record",&format!("/observation/records/{}/state",records.len()))?);
            }
            "save_snapshot" | "capture_actual_boundary" => {
                let name = string(action, "name")?;
                snapshots.insert(name.into(), state.clone());
                records.push(capture::root_point(json!({"ordinal":ordinal,"op":op,"name":name,"state":projection::state(&state,&aliases)?}),&state,&aliases,"root::run actual scoped record",&format!("/observation/records/{}/state",records.len()))?);
            }
            "attempt_effect_frame_reply_without_adoption" if extended => {
                let main_before = state.clone();
                let source = snapshots
                    .get(string(action, "snapshot")?)
                    .ok_or("unknown private frame source")?;
                let (actual, witness) =
                    extended::wrong_wave(source, action, &selected.fixture, &joins, &aliases)?;
                records.push(capture::effect_point(json!({"ordinal":ordinal,"op":op,"actual":actual,"retained_main_source_full_state_eq":state==main_before}),source.execution(),&aliases,"root::run actual private frame source")?);
                frame_attempts.push(witness);
            }
            _ => {
                let (source, adopted, inputs) = match op {
                    "attempt_without_adoption" => {
                        let source = snapshots
                            .get(string(action, "snapshot")?)
                            .ok_or("unknown attempt source")?
                            .clone();
                        let input =
                            command(&source, &action["command"], &selected.fixture, &joins)?;
                        (source, false, vec![input])
                    }
                    "attempt_reply_without_adoption" => {
                        let source = snapshots
                            .get(string(action, "snapshot")?)
                            .ok_or("unknown reply source")?
                            .clone();
                        let input = reply(&source, &action["mutation"], &selected.fixture, &joins)?;
                        (source, false, vec![input])
                    }
                    "pass_round" => {
                        let actors = rows(action, "actors")?;
                        if actors.is_empty() || actors.len() > 6 {
                            return Err("invalid finite pass round".into());
                        }
                        // Each subsequent input is bound only after its actual predecessor.
                        let mut next = state.clone();
                        for actor in actors {
                            let input = GameInput {
                                basis: next.basis.clone(),
                                actor: packet::seat(actor.as_str().ok_or("nonstring pass actor")?)?,
                                command: GameCommand::Pass,
                            };
                            let before = next.clone();
                            let result =
                                crate::test_support::current_capture::advance(&next, input.clone());
                            let retained = next == before;
                            records.push(capture::root_point(json!({"ordinal":ordinal,"op":op,"adopt_requested":true,"input":projection::input(&input),"source":projection::state(&next,&aliases)?,"retained_source_full_state_eq":retained,"result":match &result {Ok(value)=>json!({"tag":"Ok","candidate":projection::candidate(value,Some(&next),&aliases)?}),Err(error)=>projection::game_error(error)}}),&next,&aliases,"root::run actual scoped record",&format!("/observation/records/{}/source",records.len()))?);
                            attempts.push((next.clone(), input, result.clone()));
                            if !retained {
                                state = next;
                                failure = Some("borrowed pass source mutated".into());
                                failed_ordinal = Some(ordinal);
                                break 'actions;
                            }
                            let candidate = match result {
                                Ok(value) => value,
                                Err(error) => {
                                    state = next;
                                    failure = Some(format!("adopted pass failed: {error:?}"));
                                    failed_ordinal = Some(ordinal);
                                    break 'actions;
                                }
                            };
                            let observed = joins
                                .observe(&selected.fixture, candidate.rules_journal())
                                .and_then(|()| {
                                    if extended {
                                        joins.observe_root(ordinal, None, &candidate)
                                    } else {
                                        Ok(())
                                    }
                                });
                            next = candidate.into_state();
                            if let Err(error) = observed {
                                state = next;
                                failure = Some(error);
                                failed_ordinal = Some(ordinal);
                                break 'actions;
                            }
                        }
                        state = next;
                        completed_ordinal = ordinal;
                        continue;
                    }
                    "reply_exact_current_frame" => (
                        state.clone(),
                        true,
                        vec![reply(&state, action, &selected.fixture, &joins)?],
                    ),
                    "activate_mana" | "cast" | "pass" => (
                        state.clone(),
                        true,
                        vec![command(&state, action, &selected.fixture, &joins)?],
                    ),
                    _ => return Err(format!("unsupported private root operation {op}")),
                };
                let [input] = inputs.as_slice() else {
                    return Err("private call requires one actual input".into());
                };
                let before = source.clone();
                let current_before = state.clone();
                let result = crate::test_support::current_capture::advance_with_adoption(
                    &source,
                    input.clone(),
                    adopted,
                );
                let source_eq = source == before;
                let current_eq = state == current_before;
                records.push(capture::root_point(json!({"ordinal":ordinal,"op":op,"adopt_requested":adopted,"input":projection::input(input),"source":projection::state(&source,&aliases)?,"result":match &result {Ok(value)=>json!({"tag":"Ok","candidate":projection::candidate(value,Some(&source),&aliases)?}),Err(error)=>projection::game_error(error)},"retained_source_full_state_eq":source_eq,"retained_main_source_full_state_eq":current_eq}),&source,&aliases,"root::run actual scoped record",&format!("/observation/records/{}/source",records.len()))?);
                attempts.push((source, input.clone(), result.clone()));
                if !source_eq || !current_eq {
                    return Err("borrowed source mutated during actual call".into());
                }
                if adopted {
                    let candidate = match result {
                        Ok(value) => value,
                        Err(error) => {
                            failure = Some(format!("adopted operation failed: {error:?}"));
                            failed_ordinal = Some(ordinal);
                            break 'actions;
                        }
                    };
                    let observed = joins
                        .observe(&selected.fixture, candidate.rules_journal())
                        .and_then(|()| {
                            if extended {
                                joins.observe_root(ordinal, Some(action), &candidate)
                            } else {
                                Ok(())
                            }
                        });
                    state = candidate.into_state();
                    if let Err(error) = observed {
                        failure = Some(error);
                        failed_ordinal = Some(ordinal);
                        break 'actions;
                    }
                }
            }
        }
        completed_ordinal = ordinal;
    }
    if failure.is_none() {
        failure = joins.require_token_bindings(&selected.fixture).err();
    }
    Ok((
        capture::root_point(
            json!({"status":if failure.is_none(){"completed_recipe"}else{"failed"},"failure":failure,"scope":selected.fixture["scope"],"records":records,"final_state":projection::state(&state,&aliases)?,"completed_action_ordinal":completed_ordinal,"failed_action_ordinal":failed_ordinal,"stop_reached":failure.is_none()&&completed_ordinal==actions.len(),"stop":selected.fixture["stop"]}),
            &state,
            &aliases,
            "root::run actual final state",
            "/observation/final_state",
        )?,
        Witness::Root {
            initialization: Box::new(Ok(initialization)),
            attempts,
            final_state: Some(Box::new(state)),
        },
        frame_attempts,
    ))
}
