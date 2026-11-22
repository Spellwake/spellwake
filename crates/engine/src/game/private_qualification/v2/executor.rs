//! Finite actual EffectWindow calls; no scheduler, interpreter or Ready shortcut.
use super::*;
use crate::{
    draw::DrawJournal,
    object_identity::{GameObjects, ObjectIdentityError},
    occurrence::RulesTransition,
};

mod tests;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum EffectAttempt {
    ResolutionPreparation(
        Box<Result<(EffectWindow, crate::stack_window::StackWindow), EffectError>>,
    ),
    PrimitiveChange(Box<Result<crate::object_identity::ZoneMoveResult, ObjectIdentityError>>),
    Cast(Box<Result<crate::effects::CastTransition, EffectError>>),
    Activation(Box<Result<crate::activated::AbilityActivationTransition, EffectError>>),
    Mana(Box<Result<crate::effects::ManaActivationProgress, EffectError>>),
    Moves(Box<Result<RulesTransition<EffectWindow, Vec<ObjectMovement>>, EffectError>>),
    Controller(Box<Result<(EffectWindow, crate::object_identity::ObjectChange), EffectError>>),
    Unit(Box<Result<RulesTransition<EffectWindow, ()>, EffectError>>),
    Draw(Box<Result<RulesTransition<EffectWindow, DrawJournal>, EffectError>>),
    Progress(Box<Result<RuleProgress, EffectError>>),
    Preparation(Box<Result<EffectWindow, EffectError>>),
    PrimitiveMoves(
        Box<
            Result<
                (GameObjects, Vec<crate::object_identity::ZoneMoveOutcome>),
                ObjectIdentityError,
            >,
        >,
    ),
}
impl EffectAttempt {
    pub(super) fn successor(&self) -> Option<&EffectWindow> {
        match self {
            Self::ResolutionPreparation(value) => {
                value.as_ref().as_ref().ok().map(|(state, _)| state)
            }
            Self::Cast(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Activation(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Mana(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Moves(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Controller(value) => value.as_ref().as_ref().ok().map(|(state, _)| state),
            Self::Unit(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Draw(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Progress(value) => value.as_ref().as_ref().ok().map(RulesTransition::state),
            Self::Preparation(value) => value.as_ref().as_ref().ok(),
            Self::PrimitiveMoves(_) | Self::PrimitiveChange(_) => None,
        }
    }
    pub(super) fn primitive_successor(&self) -> Option<&GameObjects> {
        match self {
            Self::PrimitiveChange(value) => value.as_ref().as_ref().ok().map(|value| &value.next),
            Self::PrimitiveMoves(value) => value.as_ref().as_ref().ok().map(|value| &value.0),
            _ => None,
        }
    }
    pub(super) fn append(&self) -> Option<&crate::occurrence::RulesJournal> {
        match self {
            Self::Cast(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::Activation(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::Mana(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::Moves(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::Unit(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::Draw(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::Progress(value) => value.as_ref().as_ref().ok().map(RulesTransition::append),
            Self::ResolutionPreparation(_)
            | Self::Controller(_)
            | Self::Preparation(_)
            | Self::PrimitiveMoves(_)
            | Self::PrimitiveChange(_) => None,
        }
    }
    pub(super) fn view(
        &self,
        aliases: &packet::AliasContext,
        seats: &[SeatId],
    ) -> Result<Value, String> {
        Ok(match self {
            Self::ResolutionPreparation(value) => match value.as_ref() {
                Ok((state, gate)) => {
                    json!({"tag":"Ok","state":projection::effect(state,aliases)?,"actual_gate_result":{"tag":"Ok","window":projection::stack_window(gate)},"scope":"explicit-private-cursor-after-real-target-gate-no-public-pause"})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::PrimitiveChange(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"outcome":projection::zone_move_outcome(&value.outcome),"primitive_has_no_publisher":true})
                }
                Err(error) => projection::object_identity_error(error),
            },
            Self::Cast(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"payment":projection::payment(value.result()),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Activation(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"activation":projection::ability_activation(&value.result().activation),"payment":projection::payment(&value.result().payment),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Mana(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"result":match value.result() {crate::effects::ManaActivationResult::Complete {production}=>json!({"tag":"Complete","production":projection::production(production)}),crate::effects::ManaActivationResult::Pending {frame}=>json!({"tag":"Pending","frame":{"source":projection::opaque(frame.source()),"ability":crate::test_support::mana_ref::view(frame.ability()),"chooser":frame.chooser().0,"amount":frame.amount(),"colors":frame.colors().iter().map(projection::opaque).collect::<Vec<_>>()}}),crate::effects::ManaActivationResult::PendingRule {frame}=>json!({"tag":"PendingRule","frame":projection::effect_frame(frame,value.state())?})},"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Moves(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"movements":value.result().iter().map(|value|json!({"before":projection::object(&value.before),"after":projection::object(&value.after)})).collect::<Vec<_>>(),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Controller(value) => match value.as_ref() {
                Ok((state, receipt)) => {
                    json!({"tag":"Ok","state":projection::effect(state,aliases)?,"primitive_receipt":{"next":projection::card_objects(&receipt.next,seats),"before":receipt.before.as_ref().map(projection::object),"after":projection::object(&receipt.after)},"scope":"explicit-private-registry-successor-no-outer-operation-or-event"})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Unit(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Draw(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"draws":projection::draws(value.result()),"append":projection::journal(value.append())})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Progress(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","progress":projection::rule_progress(value,aliases)?})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::Preparation(value) => match value.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::effect(value,aliases)?,"scope":"state only; operation names the actual owner method or private preparation"})
                }
                Err(error) => projection::effect_error(error),
            },
            Self::PrimitiveMoves(value) => match value.as_ref() {
                Ok((value, movements)) => {
                    json!({"tag":"Ok","state":projection::card_objects(value,seats),"movements":movements.iter().map(projection::zone_move_outcome).collect::<Vec<_>>(),"primitive_has_no_publisher":true})
                }
                Err(error) => projection::object_identity_error(error),
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ExecutorWitness {
    initialization: Box<Result<EffectWindow, EffectError>>,
    private_sources: Vec<(String, EffectWindow)>,
    attempts: Vec<(EffectWindow, EffectAttempt, bool)>,
    final_state: Option<Box<EffectWindow>>,
}

fn call(
    source: &EffectWindow,
    action: &Value,
    aliases: &packet::AliasContext,
    adopt: bool,
) -> Result<EffectAttempt, String> {
    if action["op"] != "trusted_call" || action["owner"] != "EffectWindow" {
        return Err("unsupported executor owner call".into());
    }
    let expected = source.revision();
    Ok(match string(action, "method")? {
        "enter_turn_step" => {
            let active = packet::seat(string(action, "active")?)?;
            let priority = action["priority"]
                .as_bool()
                .ok_or("missing priority bool")?;
            let empty_mana = action["empty_mana"]
                .as_bool()
                .ok_or("missing empty_mana bool")?;
            EffectAttempt::Unit(Box::new(capture::transition(
                source,
                aliases,
                projection::EffectCall::EnterTurnStep {
                    expected,
                    active,
                    priority,
                    empty_mana,
                },
                |source| source.enter_turn_step(expected, active, priority, empty_mana),
                |_| Value::Null,
                true,
            )?))
        }
        "draw_cards" => {
            let player = packet::seat(string(action, "player")?)?;
            let amount = quantity(&action["amount"])?;
            EffectAttempt::Draw(Box::new(capture::transition(
                source,
                aliases,
                projection::EffectCall::DrawCards {
                    expected,
                    player,
                    amount: &amount,
                },
                |source| source.draw_cards(expected, player, amount.clone()),
                projection::draws,
                adopt,
            )?))
        }
        "begin_checks" => EffectAttempt::Progress(Box::new(capture::progress(
            source,
            aliases,
            projection::EffectCall::BeginChecks { expected },
            |source| source.begin_checks(expected),
            adopt,
        )?)),
        _ => return Err("unsupported executor call method".into()),
    })
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if !prepared.trusted_precheck_overrides.is_empty()
        || prepared.initial_status.iter().any(|(_, tapped)| *tapped)
        || !matches!(prepared.initial, packet::Initial::TrustedUnitPrecheck {})
    {
        return Err("unsupported hidden executor preparation".into());
    }
    let initialization = crate::test_support::native_calls::effect_initialize(prepared.seed);
    let mut witness = ExecutorWitness {
        initialization: Box::new(initialization.clone()),
        private_sources: vec![],
        attempts: vec![],
        final_state: None,
    };
    let mut state = match initialization {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","initialization":projection::effect_error(&error)}),
                V2Witness::Executor(Box::new(witness)),
            ));
        }
    };
    let joins = Joins::seed(&selected.fixture, &state)?;
    let mut snapshots = BTreeMap::new();
    let mut records = vec![capture::effect_record_point(
        json!({"operation":"EffectWindow.new","result":{"tag":"Ok","state":projection::effect(&state,&prepared.aliases)?}}),
        &state,
        &prepared.aliases,
        "executor actual initialization",
        "/observation/records/0/result/state",
    )?];
    let actions = rows(&selected.fixture, "actions")?;
    if selected.fixture["stop"]["after_action_ordinal"].as_u64() != Some(actions.len() as u64)
        || selected.fixture["stop"]["no_extra_pass"] != true
    {
        return Err("unsupported executor finite stop".into());
    }
    let mut failed = None;
    for (index, action) in actions.iter().enumerate() {
        let op = string(action, "op")?;
        if op == "private_fault_clone" {
            let mut clone = state.clone();
            if action["owner"] != "GameObjects" || action["field"] != "next_identity" {
                return Err("unsupported executor private fault".into());
            }
            clone.qualification_set_fault(QualificationFault::ObjectIdentity(integer(
                &action["value"],
            )?));
            let name = string(action, "bind")?;
            snapshots.insert(name.to_owned(), clone.clone());
            witness
                .private_sources
                .push((name.to_owned(), clone.clone()));
            records.push(capture::add_effect_record_point(capture::effect_record_point(json!({"ordinal":index+1,"operation":action,"source":projection::effect(&state,&prepared.aliases)?,"private_clone":projection::effect(&clone,&prepared.aliases)?}),&state,&prepared.aliases,"executor::run actual scoped record",&format!("/observation/records/{}/source",records.len()))?,&clone,&prepared.aliases,"executor actual privately faulted clone",&format!("/observation/records/{}/private_clone",records.len()))?);
            continue;
        }
        let (source, before, result, adopt) = match op {
            "trusted_call" => {
                let source = state.clone();
                let before = source.clone();
                let adopt = action["adopt"].as_bool().unwrap_or(true);
                let result = call(&source, action, &prepared.aliases, adopt)?;
                (source, before, result, adopt)
            }
            "private_prepare_fields" => {
                let counters = action
                    .get("pt_counters")
                    .map(|_| rows(action, "pt_counters"))
                    .transpose()?
                    .unwrap_or(&[])
                    .iter()
                    .map(|row| {
                        Ok((
                            joins.object(&row["object"], &selected.fixture, &state)?,
                            PowerToughnessCounters {
                                plus_one: quantity(&row["plus_one"])?,
                                minus_one: quantity(&row["minus_one"])?,
                            },
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let marked = action
                    .get("marked_damage")
                    .map(|_| rows(action, "marked_damage"))
                    .transpose()?
                    .unwrap_or(&[])
                    .iter()
                    .map(|row| {
                        Ok((
                            joins.object(&row["object"], &selected.fixture, &state)?,
                            quantity(&row["amount"])?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if action.get("life_totals").is_some() || action.get("combat").is_some() {
                    return Err("unsupported private preparation field".into());
                }
                let source = state.clone();
                let before = source.clone();
                let result = EffectAttempt::Preparation(Box::new(capture::state(
                    &source,
                    &prepared.aliases,
                    projection::EffectCall::QualificationPrepareCreatureFields {
                        counters: &counters,
                        marked: &marked,
                    },
                    |source| source.qualification_prepare_creature_fields(&counters, &marked),
                    true,
                )?));
                (source, before, result, true)
            }
            "attempt_owner_call_without_adoption" => {
                let source = snapshots
                    .get(string(action, "snapshot")?)
                    .ok_or("unknown executor private source")?
                    .clone();
                if action["owner"] != "GameObjects" || action["method"] != "apply_zone_moves" {
                    return Err("unsupported executor raw owner probe".into());
                }
                let moves = rows(action, "moves")?
                    .iter()
                    .map(|row| {
                        if row["destination"] != "Graveyard" {
                            return Err("unsupported raw batch destination".into());
                        }
                        Ok((
                            joins.object(&row["object"], &selected.fixture, &source)?,
                            ObjectDestination::Graveyard,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let before = source.clone();
                let registry = source.window().cards();
                let expected = registry.revision();
                let seats = source.window().participants();
                let result = capture::card(
                    registry,
                    seats,
                    projection::GameObjectsCall::ApplyZoneMoves {
                        expected,
                        moves: &moves,
                    },
                    |registry| registry.apply_zone_moves(expected, &moves),
                    |result| {
                        Ok(match result {
                            Ok((cards, movements)) => {
                                json!({"tag":"Ok","state":projection::card_objects(cards,seats),"movements":movements.iter().map(projection::zone_move_outcome).collect::<Vec<_>>()})
                            }
                            Err(error) => projection::object_identity_error(error),
                        })
                    },
                    |value| Some(&value.0),
                    false,
                )?;
                (
                    source,
                    before,
                    EffectAttempt::PrimitiveMoves(Box::new(result)),
                    false,
                )
            }
            _ => return Err("unsupported private executor operation".into()),
        };
        let actual = result.view(&prepared.aliases, state.window().participants())?;
        let next = result.successor().cloned();
        records.push(capture::effect_record_point(json!({"ordinal":index+1,"operation":action,"source":projection::effect(&source,&prepared.aliases)?,"result":actual,"adopt_requested":adopt,"retained_source_full_state_eq":source==before}),&source,&prepared.aliases,"executor::run actual scoped record",&format!("/observation/records/{}/source",records.len()))?);
        witness.attempts.push((source, result, adopt));
        if adopt {
            if let Some(next) = next {
                state = next;
            } else {
                failed = Some(index + 1);
                break;
            }
        }
    }
    witness.final_state = Some(Box::new(state.clone()));
    Ok((
        capture::effect_record_point(
            json!({"status":if failed.is_none(){"completed_recipe"}else{"failed"},"scope":selected.fixture["scope"],"records":records,"failed_action_ordinal":failed,"stop_reached":failed.is_none(),"final_state":projection::effect(&state,&prepared.aliases)?,"stop":selected.fixture["stop"]}),
            &state,
            &prepared.aliases,
            "executor actual final state",
            "/observation/final_state",
        )?,
        V2Witness::Executor(Box::new(witness)),
    ))
}
