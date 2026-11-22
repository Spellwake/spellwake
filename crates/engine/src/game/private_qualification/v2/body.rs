//! Finite callers of the existing action, target gate and instruction driver.
use super::*;
use crate::{
    damage::{DamageApplication, DamageEffectCreation, DamageEffectId, DamageOrigin},
    occurrence::{RulesJournal, RulesOccurrence},
    stack_window::StackEntryId,
};
use executor::EffectAttempt;

mod actions;
mod operations;
mod resolution_values;
pub(super) use resolution_values::root_faults::RootWitness as ResolutionRootWitness;
#[cfg(test)]
mod player_target_tests;
mod tests;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct BodyWitness {
    initialization: Box<Result<EffectWindow, EffectError>>,
    attempts: Vec<(EffectWindow, EffectAttempt, bool)>,
    repeated_attempts: Vec<EffectAttempt>,
    final_state: Option<Box<EffectWindow>>,
    calendars: Vec<(Option<TurnScheduler>, Result<TurnScheduler, TurnError>)>,
    raw_branches: Vec<raw_window::RawResolutionWitness>,
}

struct Caller<'a> {
    fixture: &'a Value,
    aliases: &'a packet::AliasContext,
    state: EffectWindow,
    joins: Joins,
    frame: Option<EffectDecision>,
    last_append: RulesJournal,
    creations: BTreeMap<(StackEntryId, usize), DamageEffectId>,
    bound_damage_attempts: BTreeMap<String, crate::damage::DamageAttemptId>,
    records: Vec<Value>,
    attempts: Vec<(EffectWindow, EffectAttempt, bool)>,
    repeated_attempts: Vec<EffectAttempt>,
    ordinal: usize,
    turn: Option<TurnScheduler>,
    completion: Option<RuleCompletion>,
    calendars: Vec<(Option<TurnScheduler>, Result<TurnScheduler, TurnError>)>,
    raw_branches: Vec<raw_window::RawResolutionWitness>,
    original_seed: &'a GameSeed,
}

impl Caller<'_> {
    /// Capture operands from their actual typed variables before the native call.
    fn invoke_with_input(
        &mut self,
        operation: Value,
        input: projection::QualificationCall<'_>,
        call: impl Fn(&EffectWindow) -> EffectAttempt,
        adopt: bool,
    ) -> Result<(), String> {
        use crate::test_support::{
            current_capture,
            current_profile::{
                Append, CallPoint, Phase, ReplayEvidence, SnapshotOwner, SourceCapture,
            },
        };
        let observed_input =
            projection::qualification_call(&input, self.aliases, Some(&self.state))?;
        let scoped_record_index = self.records.len();
        let child_card_owner = matches!(&input, projection::QualificationCall::GameObjectsCall(_));
        let capture = if current_capture::is_active() {
            let id = current_capture::native_call_id()?;
            let owner = match &input {
                projection::QualificationCall::GameObjectsCall(_) => SnapshotOwner::GameObjects {
                    cards: self.state.window().cards(),
                    participants: self.state.window().participants(),
                },
                projection::QualificationCall::EffectCall(_) => {
                    SnapshotOwner::EffectWindow(&self.state)
                }
                _ => {
                    return Err(
                        "body caller descriptor does not match an actual existing owner".into(),
                    );
                }
            };
            let before = format!("body-source-{id}");
            current_capture::snapshot_at(
                &before,
                &[],
                Phase::BeforeCall,
                input.owner_method(),
                owner,
                self.aliases,
            )?;
            if !child_card_owner {
                current_capture::reference_record_point(
                    &before,
                    &format!("/observation/records/{scoped_record_index}/source"),
                )?;
            }
            Some((id, before))
        } else {
            None
        };
        let scoped_before_point = capture.as_ref().map(|(_, point)| point.clone());
        let source = self.state.clone();
        let result = call(&self.state);
        let retained = self.state == source;
        let repeated = call(&self.state);
        let repeated_eq = result == repeated;
        let retained_after_replay = self.state == source;
        let actual = result.view(self.aliases, self.state.window().participants());
        let projection_failure = actual.as_ref().err().cloned();
        let actual=actual.unwrap_or_else(|error|json!({"tag":"ObservationProjectionFailure","failure_domain":"DriverOrCodec","failure":error,"actual_native_debug":format!("{result:?}")}));
        if let Some((id, before)) = capture {
            let after = if let Some(state) = result.successor() {
                Some(current_capture::native_snapshot(
                    Phase::AfterCall,
                    input.owner_method(),
                    SnapshotOwner::EffectWindow(state),
                    self.aliases,
                )?)
            } else if let Some(cards) = result.primitive_successor() {
                Some(current_capture::native_snapshot(
                    Phase::AfterCall,
                    input.owner_method(),
                    SnapshotOwner::GameObjects {
                        cards,
                        participants: self.state.window().participants(),
                    },
                    self.aliases,
                )?)
            } else {
                None
            };
            let append = result
                .append()
                .map(Append::RulesJournal)
                .unwrap_or(Append::NotApplicable);
            macro_rules! record_result {
                ($first:expr,$second:expr) => {
                    current_capture::record_native(
                        CallPoint {
                            call_id: &id,
                            before_point: Some(&before),
                            after_point: after.as_deref(),
                            adopted: adopt && $first.is_ok(),
                        },
                        &input,
                        $first,
                        $second,
                        |_| Ok(actual.clone()),
                        append,
                        ReplayEvidence {
                            source_eq: Some(retained && retained_after_replay),
                            source_capture: SourceCapture::BeforeInvocation,
                        },
                        self.aliases,
                        Some(&self.state),
                    )?
                };
            }
            match (&result, &repeated) {
                (
                    EffectAttempt::ResolutionPreparation(left),
                    EffectAttempt::ResolutionPreparation(right),
                ) => record_result!(left.as_ref(), right.as_ref()),
                (EffectAttempt::PrimitiveChange(left), EffectAttempt::PrimitiveChange(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Cast(left), EffectAttempt::Cast(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Activation(left), EffectAttempt::Activation(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Mana(left), EffectAttempt::Mana(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Moves(left), EffectAttempt::Moves(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Controller(left), EffectAttempt::Controller(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Unit(left), EffectAttempt::Unit(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Draw(left), EffectAttempt::Draw(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Progress(left), EffectAttempt::Progress(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::Preparation(left), EffectAttempt::Preparation(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                (EffectAttempt::PrimitiveMoves(left), EffectAttempt::PrimitiveMoves(right)) => {
                    record_result!(left.as_ref(), right.as_ref())
                }
                _ => {
                    return Err(
                        "same typed native caller returned a different Result variant".into(),
                    );
                }
            }
        }
        let successor = result.successor().cloned();
        let mut join_failure = None;
        if adopt {
            if let EffectAttempt::Progress(value) = &result
                && let Ok(value) = value.as_ref()
            {
                self.frame = match value.result() {
                    RuleResult::Complete { .. } => None,
                    RuleResult::Pending { frame, .. } => Some(frame.clone()),
                };
                self.completion = match value.result() {
                    RuleResult::Complete { completion, .. } => Some(*completion),
                    RuleResult::Pending { .. } => None,
                };
            }
            self.last_append = result.append().cloned().unwrap_or_default();
            for record in &self.last_append.records {
                if let RulesRecord::Event(event) = record {
                    for occurrence in &event.occurrences {
                        match &occurrence.value {
                            RulesOccurrence::DamageEffectCreated(
                                DamageEffectCreation::Prevention {
                                    shield,
                                    instruction,
                                },
                            ) => {
                                self.creations
                                    .insert((shield.creator, *instruction), shield.id.into());
                            }
                            RulesOccurrence::DamageEffectCreated(
                                DamageEffectCreation::Multiplier {
                                    multiplier,
                                    instruction,
                                },
                            ) => {
                                self.creations.insert(
                                    (multiplier.creator, *instruction),
                                    multiplier.id.into(),
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
            join_failure = self.joins.observe(self.fixture, &self.last_append).err();
        }
        let source_view = projection::effect(&source, self.aliases).unwrap_or_else(
            |error| json!({"projection_failure":error,"actual_native_debug":format!("{source:?}")}),
        );
        if self.records.len() != scoped_record_index {
            return Err("actual body writer index changed during invocation".into());
        }
        let mut record = capture::attach_writer_point(
            json!({"action_ordinal":self.ordinal,"operation":operation,"native_call_input":observed_input,"source":source_view,"result":actual,"adopt_requested":adopt,"retained_source_full_state_eq":retained,"same_source_typed_result_replay_eq":repeated_eq,"retained_source_after_replay_eq":retained_after_replay,"join_failure":join_failure,"projection_failure":projection_failure}),
            scoped_before_point,
        );
        if child_card_owner {
            record = capture::add_effect_record_point(
                record,
                &source,
                self.aliases,
                "body actual enclosing executor source",
                &format!("/observation/records/{scoped_record_index}/source"),
            )?;
        }
        self.records.push(record);
        self.attempts.push((source, result, adopt));
        self.repeated_attempts.push(repeated);
        if !retained || !retained_after_replay || !repeated_eq {
            return Err("borrowed actual executor source mutated".into());
        }
        if adopt {
            self.state = successor
                .ok_or("prescribed actual executor call rejected; actual Result retained")?;
        }
        if let Some(error) = projection_failure.or(join_failure) {
            return Err(error);
        }
        Ok(())
    }
    fn checks(&mut self, label: &str) -> Result<(), String> {
        self.invoke_with_input(
            json!({"owner":"EffectWindow","method":"begin_checks","role":label}),
            projection::QualificationCall::EffectCall(projection::EffectCall::BeginChecks {
                expected: self.state.revision(),
            }),
            |s| EffectAttempt::Progress(Box::new(s.begin_checks(s.revision()))),
            true,
        )
    }
    fn ready(&mut self) -> Result<(), String> {
        if matches!(
            self.state.window().phase(),
            WindowPhase::Priority(PriorityPhase::RequiresChecks { .. })
        ) {
            self.checks("actual required checks before next legal action")?;
        }
        if !matches!(
            self.state.window().phase(),
            WindowPhase::Priority(PriorityPhase::Ready { .. })
        ) {
            return Err(
                "finite recipe requires actual Ready; no synthetic completion or unlisted choice"
                    .into(),
            );
        }
        Ok(())
    }
    fn target(&self, value: &Value) -> Result<TargetRef, String> {
        self.joins.target_with_missing_entry_error(
            value,
            self.fixture,
            &self.state,
            "unbound actual entry receipt",
        )
    }
    fn payment(value: &Value) -> Result<ManaPayment, String> {
        let (rows, from_lots) = if let Some(object) = value.as_object() {
            if object.len() != 2 {
                return Err("invalid mana allocation".into());
            }
            let lots = serde_json::from_value(
                object
                    .get("from_lots")
                    .ok_or("missing mana lot selections")?
                    .clone(),
            )
            .map_err(|_| "invalid mana lot selections")?;
            (object.get("spend").ok_or("missing spend")?, lots)
        } else {
            (value, Default::default())
        };
        Ok(ManaPayment {
            from_lots,
            spend: rows
                .as_array()
                .ok_or("missing exact payment")?
                .iter()
                .map(|row| {
                    Ok(ManaSpend {
                        mana_type: program::mana(string(row, "type")?)?,
                        amount: quantity(&row["amount"])?,
                    })
                })
                .collect::<Result<_, String>>()?,
        })
    }
    fn bind_entry(&mut self, name: Option<&str>, entry: StackEntryId) -> Result<(), String> {
        if let Some(name) = name {
            self.joins.bind_entry(name, entry)?;
        }
        Ok(())
    }
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    if let Some(result) = resolution_values::run_root(selected) {
        return result;
    }
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if !matches!(prepared.initial, packet::Initial::TrustedUnitPrecheck {})
        || !prepared.trusted_precheck_overrides.is_empty()
        || prepared.initial_status.iter().any(|(_, value)| *value)
    {
        return Err("body unit cannot hide initial preparation".into());
    }
    let initialization =
        crate::test_support::native_calls::effect_initialize(prepared.seed.clone());
    let state = match &initialization {
        Ok(value) => value.clone(),
        Err(error) => {
            return Ok((
                json!({"status":"failed","initialization":projection::effect_error(error)}),
                V2Witness::Body(Box::new(BodyWitness {
                    initialization: Box::new(initialization),
                    attempts: vec![],
                    repeated_attempts: vec![],
                    final_state: None,
                    calendars: vec![],
                    raw_branches: vec![],
                })),
            ));
        }
    };
    let joins = Joins::seed(&selected.fixture, &state)?;
    let mut caller = Caller {
        fixture: &selected.fixture,
        aliases: &prepared.aliases,
        state,
        joins,
        frame: None,
        last_append: RulesJournal::default(),
        creations: BTreeMap::new(),
        bound_damage_attempts: BTreeMap::new(),
        records: vec![],
        attempts: vec![],
        repeated_attempts: vec![],
        ordinal: 0,
        turn: None,
        completion: None,
        calendars: vec![],
        raw_branches: vec![],
        original_seed: &prepared.seed,
    };
    let actions = rows(&selected.fixture, "actions")?;
    if selected.fixture["stop"]["after_action_ordinal"].as_u64() != Some(actions.len() as u64)
        || selected.fixture["stop"]["no_extra_pass"] != true
    {
        return Err("unsupported body finite stop".into());
    }
    let mut failure = None;
    for (index, action) in actions.iter().enumerate() {
        caller.ordinal = index + 1;
        if let Err(error) = caller.action(action) {
            failure = Some(error);
            break;
        }
    }
    if failure.is_none() {
        failure = caller.joins.require_token_bindings(caller.fixture).err();
    }
    let report = capture::effect_record_point(
        json!({"status":if failure.is_none(){"completed_recipe"}else{"failed"},"scope":selected.fixture["scope"],"initialization":{"tag":"Ok","state":projection::effect(initialization.as_ref().unwrap(),&prepared.aliases)?},"records":caller.records,"failure":failure,"failed_action_ordinal":failure.as_ref().map(|_|caller.ordinal),"stop_reached":failure.is_none(),"final_state":projection::effect(&caller.state,&prepared.aliases)?,"final_frame":caller.frame.as_ref().map(|frame|projection::effect_frame(frame,&caller.state)).transpose()?,"final_calendar":caller.turn.as_ref().map(|value|format!("{value:?}")),"stop":selected.fixture["stop"]}),
        &caller.state,
        &prepared.aliases,
        "body actual final state",
        "/observation/final_state",
    )?;
    Ok((
        report,
        V2Witness::Body(Box::new(BodyWitness {
            initialization: Box::new(initialization),
            attempts: caller.attempts,
            repeated_attempts: caller.repeated_attempts,
            final_state: Some(Box::new(caller.state)),
            calendars: caller.calendars,
            raw_branches: caller.raw_branches,
        })),
    ))
}
