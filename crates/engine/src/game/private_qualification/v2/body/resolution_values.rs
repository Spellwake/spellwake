//! Finite trusted S2 variants over the existing actual-owner caller.
use super::*;
use crate::damage::{DamageAllocation, DamageInput, DamageJournal, DamageKind, DamageRecipient};
use crate::draw::DrawJournal;
use crate::effects::private_qualification::QualificationFault;
use projection::{EffectCall, QualificationCall};
use spellwake_contracts::creature_types::{CreatureTypeId, CreatureTypeVocabulary};

mod guards;
pub(super) mod root_faults;

pub(super) fn run_root(
    selected: &packet::SelectedInput,
) -> Option<Result<(Value, V2Witness), String>> {
    root_faults::accepts(selected.fixture["id"].as_str()?).then(|| root_faults::run(selected))
}

fn selected_type(caller: &Caller<'_>, lookup: &Value) -> Result<CreatureTypeId, String> {
    let label = string(lookup, "label")?;
    let value = match string(lookup, "vocabulary")? {
        "Vfull" => caller.state.creature_vocabulary().resolve_label(label),
        "Vforeign" => {
            // These are the explicit synthetic inputs selected in identity_contract.
            let foreign = CreatureTypeVocabulary::new(
                "SYNTHETIC-SHAPE-FOREIGN".into(),
                [0; 32],
                caller.state.creature_vocabulary().members().to_vec(),
            )
            .map_err(|error| format!("foreign vocabulary construction: {error:?}"))?;
            foreign.resolve_label(label)
        }
        _ => return Err("unlisted typed vocabulary lookup".into()),
    }
    .ok_or("selected literal label has no actual typed member")?;
    if value.ordinal().to_string() != string(lookup, "ordinal")? {
        return Err("literal ordinal does not match actual lookup".into());
    }
    Ok(value)
}

fn invoke(caller: &mut Caller<'_>, action: &Value) -> Result<(), String> {
    let expected = caller.state.revision();
    let adopt = action["adopt"]
        .as_bool()
        .ok_or("missing explicit adoption")?;
    let method = string(action, "method")?;
    match method {
        "enter_turn_step" => {
            let active = packet::seat(string(action, "active")?)?;
            let priority = action["priority"]
                .as_bool()
                .ok_or("missing priority flag")?;
            let empty_mana = action["empty_mana"]
                .as_bool()
                .ok_or("missing mana-empty flag")?;
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::EnterTurnStep {
                    expected,
                    active,
                    priority,
                    empty_mana,
                }),
                |source| {
                    EffectAttempt::Unit(Box::new(
                        source.enter_turn_step(expected, active, priority, empty_mana),
                    ))
                },
                adopt,
            )
        }
        "begin_checks" => {
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::BeginChecks { expected }),
                |source| EffectAttempt::Progress(Box::new(source.begin_checks(expected))),
                adopt,
            )?;
            if let Some(name) = action["bind_placement"].as_str() {
                let placed = caller
                    .last_append
                    .records
                    .iter()
                    .filter_map(|record| match record {
                        RulesRecord::TriggerPlaced(value) => Some(value.entry),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let [entry] = placed.as_slice() else {
                    return Err("expected actual singleton trigger placement".into());
                };
                caller.bind_entry(Some(name), *entry)?;
            }
            Ok(())
        }
        "begin_resolution" => caller.invoke_with_input(
            action.clone(),
            QualificationCall::EffectCall(EffectCall::BeginResolution { expected }),
            |source| EffectAttempt::Progress(Box::new(source.begin_resolution(expected))),
            adopt,
        ),
        "pass" => {
            let actor = packet::seat(string(action, "actor")?)?;
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::Pass { expected, actor }),
                |source| EffectAttempt::Preparation(Box::new(source.pass(expected, actor))),
                adopt,
            )
        }
        "cast_on_turn" => {
            let actor = packet::seat(string(action, "actor")?)?;
            let spell = caller
                .joins
                .object(&action["source"], caller.fixture, &caller.state)?;
            let target = (!action["target"].is_null())
                .then(|| caller.target(&action["target"]))
                .transpose()?;
            let payment = Caller::payment(&action["payment"])?;
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::CastOnTurn {
                    expected,
                    actor,
                    spell,
                    target,
                    payment: Some(&payment),
                    turn: None,
                }),
                |source| {
                    EffectAttempt::Cast(Box::new(source.cast_on_turn(
                        expected,
                        actor,
                        spell,
                        target,
                        Some(payment.clone()),
                        None,
                    )))
                },
                adopt,
            )?;
            let Some((_, EffectAttempt::Cast(result), _)) = caller.attempts.last() else {
                return Err("missing actual cast Result".into());
            };
            let entry = result
                .as_ref()
                .as_ref()
                .map_err(|_| "actual cast rejected")?
                .result()
                .source;
            caller.bind_entry(action["bind_entry"].as_str(), entry)
        }
        "activate_ability" => {
            let actor = packet::seat(string(action, "actor")?)?;
            let source = caller
                .joins
                .object(&action["source"], caller.fixture, &caller.state)?;
            let ability = crate::activated::ActivatedAbilityId(
                action["ability_id"].as_u64().ok_or("missing ability ID")?,
            );
            let target = caller.target(&action["target"])?;
            let payment = Caller::payment(&action["payment"])?;
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::ActivateAbility {
                    expected,
                    actor,
                    source,
                    ability,
                    target,
                    payment: Some(&payment),
                }),
                |state| {
                    EffectAttempt::Activation(Box::new(state.activate_ability(
                        expected,
                        actor,
                        source,
                        ability,
                        target,
                        Some(payment.clone()),
                    )))
                },
                adopt,
            )?;
            let Some((_, EffectAttempt::Activation(result), _)) = caller.attempts.last() else {
                return Err("missing actual ability Result".into());
            };
            let entry = result
                .as_ref()
                .as_ref()
                .map_err(|_| "actual ability rejected")?
                .result()
                .activation
                .entry;
            caller.bind_entry(action["bind_entry"].as_str(), entry)
        }
        "qualification_controller_successor" => {
            let object = caller
                .joins
                .object(&action["source"], caller.fixture, &caller.state)?;
            let controller = packet::seat(string(action, "controller")?)?;
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::QualificationControllerSuccessor {
                    object,
                    controller,
                }),
                |source| {
                    EffectAttempt::Controller(Box::new(
                        source.qualification_controller_successor(object, controller),
                    ))
                },
                adopt,
            )
        }
        "qualification_zone_moves" => {
            let rows = rows(action, "moves")?;
            let moves = rows
                .iter()
                .map(|row| {
                    let object =
                        caller
                            .joins
                            .object(&row["source"], caller.fixture, &caller.state)?;
                    let destination = match &row["destination"] {
                        Value::String(value) if value == "Graveyard" => {
                            ObjectDestination::Graveyard
                        }
                        Value::String(value) if value == "Hand" => ObjectDestination::Hand,
                        Value::Object(value)
                            if value.len() == 1 && value.contains_key("Battlefield") =>
                        {
                            ObjectDestination::Battlefield {
                                controller: packet::seat(
                                    value["Battlefield"]
                                        .as_str()
                                        .ok_or("invalid battlefield controller")?,
                                )?,
                            }
                        }
                        _ => return Err("unlisted trusted destination".to_owned()),
                    };
                    Ok((object, destination))
                })
                .collect::<Result<Vec<_>, String>>()?;
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::QualificationZoneMoves {
                    expected,
                    moves: &moves,
                }),
                |source| {
                    EffectAttempt::Moves(Box::new(
                        source.qualification_zone_moves(expected, &moves),
                    ))
                },
                adopt,
            )?;
            let Some((_, EffectAttempt::Moves(result), _)) = caller.attempts.last() else {
                return Err("missing actual movement Result".into());
            };
            let movements = result
                .as_ref()
                .as_ref()
                .map_err(|_| "actual movements rejected")?
                .result();
            if movements.len() != rows.len() {
                return Err("actual movement receipt cardinality mismatch".into());
            }
            for (row, movement) in rows.iter().zip(movements) {
                if let Some(name) = row["bind_after"].as_str() {
                    caller.joins.bind_object(name, movement.after.id())?;
                }
            }
            Ok(())
        }
        "resume_rule" => {
            let frame = caller
                .frame
                .clone()
                .ok_or("no actual current saved frame")?;
            let actor = packet::seat(string(action, "actor")?)?;
            let reply = match string(&action["reply"], "kind")? {
                "TimestampOrder" => DecisionReply::TimestampOrder(
                    rows(&action["reply"], "items")?
                        .iter()
                        .map(program::token_member)
                        .collect::<Result<_, _>>()?,
                ),
                "ManaColor" => {
                    DecisionReply::ManaColor(program::mana(string(&action["reply"], "value")?)?)
                }
                "CreatureType" => DecisionReply::CreatureType(selected_type(
                    caller,
                    &action["reply"]["id_lookup"],
                )?),
                "AllocatePrevention" => DecisionReply::AllocatePrevention(
                    rows(&action["reply"], "rows")?
                        .iter()
                        .map(|row| {
                            Ok(DamageAllocation {
                                attempt: *caller
                                    .bound_damage_attempts
                                    .get(string(row, "attempt_bound")?)
                                    .ok_or("unbound actual eligible attempt")?,
                                amount: quantity(&row["amount"])?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                ),
                _ => return Err("unimplemented S2 typed reply variant".into()),
            };
            let turn = caller.turn.clone();
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::ResumeRule {
                    expected,
                    frame: &frame,
                    actor,
                    reply: &reply,
                    turn: turn.as_ref(),
                }),
                |source| {
                    EffectAttempt::Progress(Box::new(source.resume_rule(
                        expected,
                        &frame,
                        actor,
                        reply.clone(),
                        turn.as_ref(),
                    )))
                },
                adopt,
            )
        }
        _ => Err(format!("unimplemented S2 actual owner method {method}")),
    }
}

pub(super) fn action(caller: &mut Caller<'_>, action: &Value) -> Result<(), String> {
    guards::before(caller, action)?;
    action_inner(caller, action)?;
    guards::after(caller, action)
}

fn action_inner(caller: &mut Caller<'_>, action: &Value) -> Result<(), String> {
    match string(action, "op")? {
        "trusted_prepare_existing_combat_owner" => {
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::QualificationPrepareEmptyCombat),
                |source| {
                    EffectAttempt::Preparation(Box::new(
                        source.qualification_prepare_empty_combat(),
                    ))
                },
                true,
            )?;
            let constructor = &action["turn_constructor"];
            if constructor["step"] != "CombatDamage" {
                return Err("unselected prepared combat step".into());
            }
            let start = TurnStart {
                number: constructor["number"]
                    .as_u64()
                    .ok_or("missing prepared combat turn")?,
                step: Step::CombatDamage,
            };
            let two_player = constructor["two_player"]
                .as_bool()
                .ok_or("missing prepared two-player flag")?;
            let result = crate::test_support::current_capture::turn_new(start, two_player);
            caller.records.push(json!({"action_ordinal":caller.ordinal,"operation":action,"actual_turn_result":match &result {Ok(value)=>json!({"tag":"Ok","state":projection::turn_scheduler(value)}),Err(error)=>projection::turn_error(error)}}));
            caller.calendars.push((None, result.clone()));
            caller.turn = Some(
                result.map_err(|error| format!("prepared combat scheduler rejected: {error:?}"))?,
            );
            Ok(())
        }
        "trusted_begin_combat_batch" => {
            let turn = caller
                .turn
                .as_ref()
                .ok_or("missing actual combat scheduler")?;
            let origin = DamageOrigin::Combat {
                position: turn.position(),
            };
            let components = rows(action, "components")?;
            let inputs = components
                .iter()
                .map(|row| {
                    let physical = caller.joins.object(
                        &row["source_object"],
                        caller.fixture,
                        &caller.state,
                    )?;
                    let source = caller
                        .state
                        .qualification_physical_damage_source(physical)
                        .map_err(|error| format!("actual combat damage source: {error:?}"))?;
                    let target = caller.joins.object(
                        &row["recipient_creature"],
                        caller.fixture,
                        &caller.state,
                    )?;
                    Ok(DamageInput {
                        source,
                        target: DamageRecipient::Creature(target),
                        amount: quantity(&row["amount"])?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let damage = DamageJournal::default();
            let draws = DrawJournal::default();
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::BeginDamageBatch {
                    origin,
                    kind: DamageKind::Combat,
                    inputs: &inputs,
                    damage: &damage,
                    draws: &draws,
                    declarations: &[],
                    assignments: &[],
                }),
                |source| {
                    EffectAttempt::Progress(Box::new(source.clone().begin_damage_batch(
                        origin,
                        DamageKind::Combat,
                        inputs.clone(),
                        (damage.clone(), draws.clone()),
                        vec![],
                        vec![],
                    )))
                },
                true,
            )
        }
        "trusted_call" | "attempt_actual_owner" => invoke(caller, action),
        "private_fault_clone" => {
            let original = caller.state.clone();
            let value = integer(&action["value"])?;
            let fault = match (string(action, "owner")?, string(action, "field")?) {
                ("EffectWindow", "revision") => QualificationFault::Revision(value),
                ("GameObjects", "revision") => QualificationFault::ObjectRevision(value),
                ("GameObjects", "next_identity") => QualificationFault::ObjectIdentity(value),
                ("TriggerState", "next_event") => QualificationFault::Event(value),
                ("TriggerState", "next_instance") => QualificationFault::Instance(value),
                ("TriggerState", "next_wave") => QualificationFault::Wave(value),
                _ => return Err("unlisted S2 executor fault field".into()),
            };
            caller.state.qualification_set_fault(fault);
            caller.records.push(json!({"action_ordinal":caller.ordinal,"operation":action,"scope":"trusted-test-source-clone-not-accepted-input","unfaulted_source":projection::effect(&original,caller.aliases)?,"faulted_source":projection::effect(&caller.state,caller.aliases)?}));
            Ok(())
        }
        "read_actual_frame" => {
            let before = caller.state.clone();
            let first = capture::effect(
                &caller.state,
                caller.aliases,
                QualificationCall::EffectCall(EffectCall::CreatureTypeChoice),
                |source| source.creature_type_choice(),
                |result| match result {
                    Ok(frame) => projection::effect_frame(
                        &EffectDecision::CreatureType(frame.clone()),
                        &caller.state,
                    )
                    .map(|frame| json!({"tag":"Ok","frame":frame})),
                    Err(error) => Ok(projection::effect_error(error)),
                },
                |_| None,
                |_| None,
                false,
            )?;
            let second = caller.state.creature_type_choice();
            if first != second || caller.state != before {
                return Err("actual frame query changed or failed replay Eq".into());
            }
            let frame = EffectDecision::CreatureType(
                first.map_err(|error| format!("actual frame read rejected: {error:?}"))?,
            );
            caller.records.push(json!({"action_ordinal":caller.ordinal,"operation":action,"source":projection::effect(&caller.state,caller.aliases)?,"actual_frame":projection::effect_frame(&frame,&caller.state)?,"source_full_eq":true,"typed_frame_replay_eq":true}));
            caller.frame = Some(frame);
            Ok(())
        }
        "trusted_accept_one_creature_type" => {
            let Some(EffectDecision::CreatureType(frame)) = caller.frame.clone() else {
                return Err("missing actual CreatureType frame".into());
            };
            let actor = packet::seat(string(action, "actor")?)?;
            let value = selected_type(caller, &action["selected_lookup"])?;
            let expected = caller.state.revision();
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::AcceptCreatureType {
                    expected,
                    frame: &frame,
                    actor,
                    value,
                }),
                |_source| EffectAttempt::Unit(Box::new(Err(EffectError::WrongCreatureTypeFrame))),
                true,
            )
        }
        "trusted_two_source_resolution_batch" => {
            let entry = caller
                .state
                .window()
                .resolving()
                .ok_or("no actual resolving entry")?
                .id();
            let cursor = caller
                .state
                .resolution_state()
                .ok_or("no actual retained cursor")?;
            let instruction = cursor.instruction();
            let inputs = rows(action, "components")?
                .iter()
                .map(|row| {
                    let source = match string(row, "role")? {
                        "entry_component" => caller
                            .state
                            .qualification_resolution_damage_source()
                            .map_err(|error| format!("actual entry damage source: {error:?}"))?,
                        "physical_P1_component" => caller
                            .state
                            .qualification_physical_damage_source(caller.joins.object(
                                &row["source_object"],
                                caller.fixture,
                                &caller.state,
                            )?)
                            .map_err(|error| format!("actual physical damage source: {error:?}"))?,
                        _ => return Err("unlisted trusted damage component role".into()),
                    };
                    let target = caller.joins.object(
                        &row["recipient_creature"],
                        caller.fixture,
                        &caller.state,
                    )?;
                    Ok(DamageInput {
                        source,
                        target: DamageRecipient::Creature(target),
                        amount: quantity(&row["amount"])?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let origin = DamageOrigin::Resolution { entry, instruction };
            let damage = DamageJournal::default();
            let draws = DrawJournal::default();
            caller.invoke_with_input(
                action.clone(),
                QualificationCall::EffectCall(EffectCall::BeginDamageBatch {
                    origin,
                    kind: DamageKind::Noncombat,
                    inputs: &inputs,
                    damage: &damage,
                    draws: &draws,
                    declarations: &[],
                    assignments: &[],
                }),
                |source| {
                    EffectAttempt::Progress(Box::new(source.clone().begin_damage_batch(
                        origin,
                        DamageKind::Noncombat,
                        inputs.clone(),
                        (damage.clone(), draws.clone()),
                        vec![],
                        vec![],
                    )))
                },
                true,
            )
        }
        "bind_eligible_attempts" => {
            let Some(EffectDecision::Damage(crate::effects::DamageDecision::Allocation(frame))) =
                caller.frame.as_ref()
            else {
                return Err("no actual allocation frame".into());
            };
            let target = caller.joins.object(
                &json!({"captured_object":"T_seed_object"}),
                caller.fixture,
                &caller.state,
            )?;
            let entry_source = caller.state.qualification_resolution_damage_source().ok();
            for name in action["binds"]
                .as_object()
                .ok_or("missing literal attempt bindings")?
                .keys()
            {
                let source = match name.as_str() {
                    "ENTRY_ATTEMPT" => entry_source.clone().ok_or("missing actual entry source")?,
                    "P1_ATTEMPT" => caller
                        .state
                        .qualification_physical_damage_source(caller.joins.object(
                            &json!({"captured_object":"P1_seed_object"}),
                            caller.fixture,
                            &caller.state,
                        )?)
                        .map_err(|error| format!("P1 source: {error:?}"))?,
                    _ => return Err("unlisted actual attempt binding".into()),
                };
                let matches = caller
                    .state
                    .pending_damage()
                    .ok_or("missing actual batch")?
                    .components()
                    .filter(|component| {
                        component.attempt().source == source
                            && component.attempt().target == DamageRecipient::Creature(target)
                    })
                    .map(|component| component.attempt().id)
                    .collect::<Vec<_>>();
                let [attempt] = matches.as_slice() else {
                    return Err("nonunique actual source/recipient attempt".into());
                };
                if !frame.eligible().iter().any(|row| row.attempt() == *attempt) {
                    return Err("bound attempt not in actual offered allocation domain".into());
                }
                if caller
                    .bound_damage_attempts
                    .insert(name.clone(), *attempt)
                    .is_some()
                {
                    return Err("duplicate attempt binding".into());
                }
            }
            caller.records.push(json!({"action_ordinal":caller.ordinal,"operation":action,"actual_bound_attempts":caller.bound_damage_attempts.iter().map(|(name,id)|json!({"name":name,"attempt":projection::opaque(id)})).collect::<Vec<_>>(),"actual_frame":projection::effect_frame(caller.frame.as_ref().unwrap(),&caller.state)?}));
            Ok(())
        }
        _ => Err(format!(
            "unimplemented finite S2 trusted operation {}",
            string(action, "op")?
        )),
    }
}

#[test]
#[ignore = "requires explicit frozen trusted input/case/output and captured source vocabulary"]
fn resolution_value_private_recipe() {
    use sha2::{Digest, Sha256};
    use std::{env, fs, path::PathBuf};
    let input =
        PathBuf::from(env::var_os("SPELLWAKE_PRIVATE_INPUT").expect("explicit trusted input path"));
    let case = env::var("SPELLWAKE_PRIVATE_CASE").expect("explicit trusted case");
    let output = PathBuf::from(
        env::var_os("SPELLWAKE_PRIVATE_OUTPUT").expect("explicit create-new output path"),
    );
    let bytes = fs::read(&input).unwrap();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        digest,
        "1ed1a29c70dec1020a421512ce4cd886a72bb451fc4a7621dce370ba01f008b6"
    );
    let parsed = packet::Packet::parse(&bytes).unwrap();
    let selected = parsed.select(&case).unwrap();
    assert!(selected.metadata["operation_lexicon"]["selection_policy_candidate"]["allow_exact_case_ids"].as_array().unwrap().iter().any(|id|id==&case));
    let explicit =
        creature_types::load_vocabulary(&creature_types::selected_vocabulary_path().unwrap())
            .unwrap();
    assert_eq!(
        selected.vocabulary, explicit,
        "selected packet differs from actual supplied vocabulary"
    );
    let profile = "resolution-values-private-unit-v1";
    capture::begin(&input, profile, None).unwrap();
    let first = super::run(&selected);
    let repeated = crate::test_support::current_capture::without_capture(|| super::run(&selected));
    let mut record = super::super::replay(first, repeated);
    let after = format!("{:x}", Sha256::digest(fs::read(&input).unwrap()));
    assert_eq!(digest, after);
    record["case"] = json!(case);
    record["profile"] = json!(profile);
    record["selected_input"] = selected.fixture;
    record["selected_definitions"] = json!(selected.definitions);
    record["packet_metadata"] = selected.metadata;
    record["input_source"] = json!({"path":input,"sha256_read":digest,"sha256_parser_input":digest,"sha256_after":after,"bytes_read":bytes.len(),"parser_called":true,"unchanged":digest==after});
    record["producer_integrity"] =
        json!(record["native_reexecution_integrity"] == true && digest == after);
    record["status"] = json!(if record["producer_integrity"] == true
        && record["observation"]["status"] == "completed_recipe"
    {
        "completed_recipe"
    } else {
        "failed"
    });
    record["qualification_claim"] = json!(
        "none; independent expected checking and captured source/binary/input evidence required"
    );
    capture::finish(profile, &mut record);
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .unwrap();
    serde_json::to_writer_pretty(file, &record).unwrap();
    assert_eq!(
        record["status"], "completed_recipe",
        "actual failing source/Result retained in output"
    );
    assert_eq!(
        record["producer_integrity"], true,
        "full source/result/reexecution witness failed"
    );
}
