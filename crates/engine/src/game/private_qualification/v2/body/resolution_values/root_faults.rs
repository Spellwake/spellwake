//! Five frozen public-origin trajectories retain the reached native fault sources.
use super::*;
use crate::effects::private_qualification::QualificationResolutionParentFault;
use crate::game::DecisionId;

#[derive(Debug, PartialEq, Eq)]
struct RootAttempt {
    source: GameState,
    input: GameInput,
    result: Result<NativeCandidate, GameError>,
    repeated: Result<NativeCandidate, GameError>,
}
#[derive(Debug, PartialEq, Eq)]
pub(in crate::game::private_qualification::v2) struct RootWitness {
    initialization: Box<Result<NativeCandidate, GameError>>,
    calls: Vec<RootAttempt>,
    root_faults: Vec<GameState>,
    extracted_effect: Option<Box<EffectWindow>>,
    effect_fault: Option<Box<EffectWindow>>,
    original_damage_frame: Option<EffectDecision>,
    application: Option<DamageApplication>,
    effect_result: Option<Box<Result<RuleProgress, EffectError>>>,
    final_state: Box<GameState>,
}

pub(super) fn accepts(case: &str) -> bool {
    matches!(
        case,
        "S2T-PF10" | "S2T-PF11" | "S2T-PF12" | "S2T-PP01" | "S2T-PP02"
    )
}

fn type_reply(value: &Value, vocabulary: &CreatureTypeVocabulary) -> Result<DecisionReply, String> {
    if value["kind"] != "CreatureType" {
        return Err("root fault recipe requires a literal CreatureType reply".into());
    }
    let value = if let Some(raw) = value.get("id") {
        creature_types::id(raw.clone(), vocabulary)?
    } else {
        let lookup = &value["id_lookup"];
        if lookup["vocabulary"] != "Vfull" {
            return Err("root fault lookup must select the actual full vocabulary".into());
        }
        let id = vocabulary
            .resolve_label(string(lookup, "label")?)
            .ok_or("root fault type label absent")?;
        if id.ordinal().to_string() != string(lookup, "ordinal")? {
            return Err("root fault lookup ordinal differs".into());
        }
        id
    };
    Ok(DecisionReply::CreatureType(value))
}

fn guard(
    state: &GameState,
    action: &Value,
    entries: &BTreeMap<String, StackEntryId>,
) -> Result<(), String> {
    let Some(expected) = action.get("stop") else {
        return Ok(());
    };
    let actual = state
        .pending_decision()
        .ok_or("literal pending stop was not reached")?;
    let entry = *entries
        .get(string(expected, "entry_ref")?)
        .ok_or("pending guard entry was not bound by a native cast receipt")?;
    let cursor = state
        .execution()
        .resolution_state()
        .ok_or("pending stop lacks actual resolution cursor")?;
    if cursor.entry() != entry || cursor.instruction() as u64 != integer(&expected["instruction"])?
    {
        return Err("actual pending resolution cursor differs from declared stop".into());
    }
    let locals = projection::resolution_state(cursor)["locals"].clone();
    match actual.frame() {
        RuleFrame::CreatureType(frame) => {
            if expected["pending_kind"] != "CreatureType"
                || frame.entry() != entry
                || frame.instruction() as u64 != integer(&expected["instruction"])?
                || frame.slot().ordinal() as u64 != integer(&expected["slot"])?
                || frame.chooser() != packet::seat(string(expected, "chooser")?)?
                || frame.vocabulary().to_string() != string(expected, "vocabulary")?
                || projection::creature_type_frame(frame)["excluding"] != expected["excluding"]
            {
                return Err("actual creature type frame differs from declared stop".into());
            }
            if expected
                .get("locals")
                .is_some_and(|expected| *expected != locals)
            {
                return Err("actual pending locals differ from declared stop".into());
            }
        }
        RuleFrame::Damage(DamageDecision::Order(frame)) => {
            let DamageOrigin::Resolution {
                entry: actual_entry,
                instruction,
            } = *frame.origin()
            else {
                return Err("public damage stop has a different native origin".into());
            };
            if expected["kind"] != "Damage::Order"
                || actual_entry != entry
                || instruction as u64 != integer(&expected["instruction"])?
                || frame.chooser() != packet::seat(string(expected, "chooser")?)?
                || expected["parent_locals"] != locals
            {
                return Err("actual damage frame/parent differs from declared stop".into());
            }
        }
        _ => return Err("actual public pending frame has the wrong kind".into()),
    }
    Ok(())
}

fn input(
    state: &GameState,
    action: &Value,
    fixture: &Value,
    joins: &Joins,
    pending: &BTreeMap<String, DecisionId>,
) -> Result<GameInput, String> {
    let actor = packet::seat(string(action, "actor")?)?;
    let command = match string(action, "command")? {
        "Pass" => GameCommand::Pass,
        "CastUntargeted" | "CastPaid" => {
            let spell = joins.object(&action["source"], fixture, state.execution())?;
            let payment = Caller::payment(&action["payment"])?;
            if action["command"] == "CastUntargeted" {
                GameCommand::CastUntargeted {
                    spell,
                    payment: Some(payment),
                }
            } else {
                GameCommand::CastPaid {
                    spell,
                    target: joins.target(&action["target"], fixture, state.execution())?,
                    payment,
                }
            }
        }
        "Reply" => {
            let literal = string(action, "decision")?;
            let decision = if literal == "exact actual source.pending_decision().id()" {
                state
                    .pending_decision()
                    .ok_or("faulted source lacks actual pending decision")?
                    .id()
            } else {
                let name = literal
                    .strip_prefix("actual ")
                    .and_then(|value| value.strip_suffix(".id"))
                    .ok_or("unsupported saved native decision binding")?;
                *pending
                    .get(name)
                    .ok_or("selected actual DecisionId was not saved")?
            };
            GameCommand::Reply {
                decision,
                choice: type_reply(&action["reply"], state.execution().creature_vocabulary())?,
            }
        }
        _ => return Err("unlisted root-fault command".into()),
    };
    Ok(GameInput {
        basis: state.basis().clone(),
        actor,
        command,
    })
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    let case = string(&selected.fixture, "id")?;
    if !accepts(case) {
        return Err("unlisted root fault case".into());
    }
    if selected.fixture["initializer"]["owner"] != "GameState::initialize"
        || selected.fixture["initializer"]["prepare_seed_mode"] != "PreparedRoot"
    {
        return Err("root fault case must use its literal Interaction initializer".into());
    }
    let prepared = selected.prepare_seed(
        packet::SeedMode::PreparedRoot,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if !matches!(prepared.initial, packet::Initial::Interaction {})
        || !prepared.trusted_precheck_overrides.is_empty()
        || prepared.initial_status.iter().any(|(_, tapped)| *tapped)
    {
        return Err("root fault case cannot hide seed preparation".into());
    }
    let initialization = crate::test_support::current_capture::initialize(
        prepared.game,
        prepared.context,
        prepared.seed,
    );
    let candidate = initialization
        .as_ref()
        .map_err(|error| format!("actual root fault initializer rejected: {error:?}"))?;
    let mut state = candidate.state().clone();
    let mut joins = Joins::seed(&selected.fixture, state.execution())?;
    let mut witness = RootWitness {
        initialization: Box::new(initialization.clone()),
        calls: vec![],
        root_faults: vec![],
        extracted_effect: None,
        effect_fault: None,
        original_damage_frame: None,
        application: None,
        effect_result: None,
        final_state: Box::new(state.clone()),
    };
    let mut records = vec![capture::root_point(
        json!({"operation":"initialize","result":{"tag":"Ok","candidate":projection::candidate(candidate,None,&prepared.aliases)?}}),
        &state,
        &prepared.aliases,
        "S2 root actual initialization",
        "/observation/records/0/result/candidate/state",
    )?];
    let mut entries = BTreeMap::new();
    let mut pending = BTreeMap::new();
    let mut faulted_root = None;
    let mut creations = BTreeMap::new();
    let actions = rows(&selected.fixture, "actions")?;
    if selected.fixture["stop"]["after_action_ordinal"].as_u64() != Some(actions.len() as u64)
        || selected.fixture["stop"]["no_extra_pass"] != true
        || selected.fixture["stop"]["no_implicit_reply"] != true
    {
        return Err("root fault recipe lacks its exact finite stop".into());
    }
    for (index, action) in actions.iter().enumerate() {
        if action["ordinal"].as_u64() != Some((index + 1) as u64) {
            return Err("root fault action ordinal differs".into());
        }
        match string(action, "op")? {
            "advance" | "attempt_actual_owner" if action["owner"] == "GameState" => {
                let is_attempt = action["op"] == "attempt_actual_owner";
                let source = if is_attempt {
                    if action["source"] != "F" || action["method"] != "advance" {
                        return Err("unlisted faulted root call".into());
                    }
                    faulted_root
                        .as_ref()
                        .ok_or("actual root fault source missing")?
                } else {
                    &state
                };
                let native_input = input(source, action, &selected.fixture, &joins, &pending)?;
                let retained = source.clone();
                let adopt = action["adopt"]
                    .as_bool()
                    .ok_or("missing literal adoption")?;
                let result = crate::test_support::current_capture::advance_with_adoption(
                    source,
                    native_input.clone(),
                    adopt,
                );
                let repeated = crate::test_support::current_capture::without_capture(|| {
                    source.advance(native_input.clone())
                });
                let source_eq = *source == retained;
                let result_eq = result == repeated;
                let record_index = records.len();
                records.push(capture::root_point(json!({"action_ordinal":index+1,"operation":action,"input":projection::input(&native_input),"source":projection::state(source,&prepared.aliases)?,"result":match &result{Ok(value)=>json!({"tag":"Ok","candidate":projection::candidate(value,Some(source),&prepared.aliases)?}),Err(error)=>projection::game_error(error)},"retained_source_full_state_eq":source_eq,"same_source_typed_result_replay_eq":result_eq,"adopt_requested":adopt}),source,&prepared.aliases,"S2 root actual call source",&format!("/observation/records/{record_index}/source"))?);
                witness.calls.push(RootAttempt {
                    source: retained,
                    input: native_input,
                    result: result.clone(),
                    repeated,
                });
                if !source_eq || !result_eq {
                    return Err("root fault actual native replay/source Eq failed".into());
                }
                if let Ok(value) = &result {
                    if let Some(name) = action["bind_entry"].as_str() {
                        let entry = value
                            .mana_payment()
                            .ok_or("actual cast has no payment receipt")?
                            .source;
                        if entries.insert(name.to_owned(), entry).is_some() {
                            return Err("duplicate actual cast entry binding".into());
                        }
                    }
                    for record in &value.rules_journal().records {
                        if let RulesRecord::Event(event) = record {
                            for occurrence in &event.occurrences {
                                if let RulesOccurrence::DamageEffectCreated(
                                    DamageEffectCreation::Prevention {
                                        shield,
                                        instruction,
                                    },
                                ) = &occurrence.value
                                    && creations
                                        .insert((shield.creator, *instruction), shield.id)
                                        .is_some()
                                {
                                    return Err("duplicate actual shield creation binding".into());
                                }
                            }
                        }
                    }
                    joins.observe(&selected.fixture, value.rules_journal())?;
                    if let Some(name) = action["bind_pending"].as_str() {
                        let actual = value
                            .state()
                            .pending_decision()
                            .ok_or("declared pending bind lacks actual DecisionId")?;
                        if pending.insert(name.to_owned(), actual.id()).is_some() {
                            return Err("duplicate actual pending binding".into());
                        }
                    }
                    guard(value.state(), action, &entries)?;
                }
                if adopt {
                    state = result
                        .map_err(|error| format!("prescribed root adoption rejected: {error:?}"))?
                        .into_state();
                }
            }
            "private_fault_clone" => {
                if action["owner"] != "GameState"
                    || action["snapshot"] != "actual reached current source"
                    || action["bind"] != "F"
                    || action["sole_mutated_field"] != true
                {
                    return Err("unsupported root fault source/mutation".into());
                }
                let mut faulted = state.clone();
                crate::game::private_qualification::root::fault(
                    &mut faulted,
                    "GameState",
                    string(action, "field")?,
                    integer(&action["value"])?,
                )?;
                let record_index = records.len();
                records.push(capture::root_point(json!({"action_ordinal":index+1,"operation":action,"source":projection::state(&state,&prepared.aliases)?,"fault_source":projection::state(&faulted,&prepared.aliases)?}),&faulted,&prepared.aliases,"S2 root actual single-field fault clone",&format!("/observation/records/{record_index}/fault_source"))?);
                witness.root_faults.push(faulted.clone());
                faulted_root = Some(faulted);
            }
            "extract_actual_executor_and_frame" => {
                if action["bind"] != "PUBLIC_EFFECT" || action["preserve_root_predecessor"] != true
                {
                    return Err("unlisted public-origin extraction".into());
                }
                let Some(RuleFrame::Damage(frame)) =
                    state.pending_decision().map(|value| value.frame())
                else {
                    return Err("actual root predecessor is not pending damage".into());
                };
                witness.original_damage_frame = Some(EffectDecision::Damage(frame.clone()));
                witness.extracted_effect = Some(Box::new(state.execution().clone()));
                let record_index = records.len();
                records.push(capture::root_point(json!({"action_ordinal":index+1,"operation":action,"root_predecessor":projection::state(&state,&prepared.aliases)?,"execution":projection::effect(state.execution(),&prepared.aliases)?,"original_frame":projection::effect_frame(witness.original_damage_frame.as_ref().unwrap(),state.execution())?}),&state,&prepared.aliases,"S2 actual public-origin predecessor",&format!("/observation/records/{record_index}/root_predecessor"))?);
            }
            "bind_actual_damage_application" => {
                if action["bind"] != "SHIELD_APP"
                    || action["require_unique_actual_offered_application"] != true
                {
                    return Err("unlisted public damage application binding".into());
                }
                let entry = *entries
                    .get(string(&action["creation_join"], "entry")?)
                    .ok_or("actual creation entry missing")?;
                let instruction =
                    usize::try_from(integer(&action["creation_join"]["instruction"])?)
                        .map_err(|error| error.to_string())?;
                let shield = *creations
                    .get(&(entry, instruction))
                    .ok_or("actual shield creation receipt missing")?;
                let Some(EffectDecision::Damage(DamageDecision::Order(frame))) =
                    &witness.original_damage_frame
                else {
                    return Err("actual original damage-order frame missing".into());
                };
                let matches = frame
                    .applications()
                    .iter()
                    .filter(|application| application.effect == shield.into())
                    .copied()
                    .collect::<Vec<_>>();
                let [application] = matches.as_slice() else {
                    return Err(
                        "actual shield receipt does not identify one offered application".into(),
                    );
                };
                witness.application = Some(*application);
                records.push(capture::effect_point(json!({"action_ordinal":index+1,"operation":action,"application":crate::test_support::damage_trace::damage_application_view(*application)}),witness.extracted_effect.as_deref().ok_or("actual extracted executor missing")?,&prepared.aliases,"S2 actual application bound before fault")?);
            }
            "private_parent_fault_clone" => {
                if action["source"] != "PUBLIC_EFFECT"
                    || action["bind"] != "F"
                    || action["sole_mutated_field"] != true
                    || action["preserve_origin_and_actual_frame"] != true
                {
                    return Err("unlisted moved-parent fault source".into());
                }
                let source = witness
                    .extracted_effect
                    .as_deref()
                    .ok_or("actual extracted executor missing")?;
                let mut faulted = source.clone();
                let fault = match string(action, "field")? {
                    "entry" => {
                        if action["replacement"] != "actual LOW_E from its accepted cast receipt" {
                            return Err("unlisted parent-entry replacement".into());
                        }
                        QualificationResolutionParentFault::Entry(
                            *entries
                                .get("LOW_E")
                                .ok_or("actual lower accepted cast receipt missing")?,
                        )
                    }
                    "instruction" => QualificationResolutionParentFault::Instruction(
                        usize::try_from(integer(&action["replacement"])?)
                            .map_err(|error| error.to_string())?,
                    ),
                    _ => return Err("unlisted parent fault field".into()),
                };
                faulted
                    .qualification_corrupt_resolution_parent(fault)
                    .map_err(|error| {
                        format!("actual parent fault preparation rejected: {error:?}")
                    })?;
                let record_index = records.len();
                records.push(capture::effect_record_point(json!({"action_ordinal":index+1,"operation":action,"source":projection::effect(source,&prepared.aliases)?,"fault_source":projection::effect(&faulted,&prepared.aliases)?}),&faulted,&prepared.aliases,"S2 actual moved parent single-field corruption",&format!("/observation/records/{record_index}/fault_source"))?);
                witness.effect_fault = Some(Box::new(faulted));
            }
            "attempt_actual_owner" if action["owner"] == "EffectWindow" => {
                if action["method"] != "resume_rule"
                    || action["source"] != "F"
                    || action["reply"]["kind"] != "DamageOrder"
                    || action["reply"]["application_bound"] != "SHIELD_APP"
                    || !action["turn"].is_null()
                    || action["adopt"] != false
                {
                    return Err("unlisted actual moved-parent owner call".into());
                }
                let source = witness
                    .effect_fault
                    .as_deref()
                    .ok_or("actual moved-parent fault source missing")?;
                let frame = witness
                    .original_damage_frame
                    .as_ref()
                    .ok_or("original saved damage frame missing")?;
                let reply = DecisionReply::DamageOrder(
                    witness
                        .application
                        .ok_or("actual offered application missing")?,
                );
                let actor = packet::seat(string(action, "actor")?)?;
                let expected = source.revision();
                let result = capture::progress(
                    source,
                    &prepared.aliases,
                    EffectCall::ResumeRule {
                        expected,
                        frame,
                        actor,
                        reply: &reply,
                        turn: None,
                    },
                    |source| source.resume_rule(expected, frame, actor, reply.clone(), None),
                    false,
                )?;
                let record_index = records.len();
                records.push(capture::effect_record_point(json!({"action_ordinal":index+1,"operation":action,"source":projection::effect(source,&prepared.aliases)?,"original_frame":projection::effect_frame(frame,source)?,"reply":projection::reply(&reply),"result":match &result{Ok(value)=>json!({"tag":"Ok","progress":projection::rule_progress(value,&prepared.aliases)?}),Err(error)=>projection::effect_error(error)}}),source,&prepared.aliases,"S2 actual moved-parent owner source",&format!("/observation/records/{record_index}/source"))?);
                witness.effect_result = Some(Box::new(result));
            }
            _ => return Err("unlisted finite root-fault operation".into()),
        }
    }
    witness.final_state = Box::new(state.clone());
    let report = capture::root_point(
        json!({"status":"completed_recipe","scope":selected.fixture["scope"],"records":records,"final_state":projection::state(&state,&prepared.aliases)?,"stop":selected.fixture["stop"],"stop_reached":true,"no_extra_pass_or_implicit_reply":true}),
        &state,
        &prepared.aliases,
        "S2 actual retained final root",
        "/observation/final_state",
    )?;
    Ok((report, V2Witness::ResolutionRoot(Box::new(witness))))
}
