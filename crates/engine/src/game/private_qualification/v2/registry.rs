//! Standalone physical-owner calls, with no executor registry substitution.
use super::*;
use crate::{
    object_identity::{GameObjects, ObjectChange, ObjectIdentityError, ObjectStatusChange},
    shuffle::{EntropyTape, LibraryShuffle, ShuffleRecord},
};

mod tests;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RegistryResult {
    Change(Box<Result<ObjectChange, ObjectIdentityError>>),
    Move(Box<Result<crate::object_identity::ZoneMoveResult, ObjectIdentityError>>),
    Registry(Box<Result<GameObjects, ObjectIdentityError>>),
    Status(Box<Result<(GameObjects, Vec<ObjectStatusChange>), ObjectIdentityError>>),
    Shuffle(Box<Result<LibraryShuffle, ObjectIdentityError>>),
}

impl RegistryResult {
    fn successor(&self) -> Option<&GameObjects> {
        match self {
            Self::Change(result) => result.as_ref().as_ref().ok().map(|value| &value.next),
            Self::Move(result) => result.as_ref().as_ref().ok().map(|value| &value.next),
            Self::Registry(result) => result.as_ref().as_ref().ok(),
            Self::Status(result) => result.as_ref().as_ref().ok().map(|(value, _)| value),
            Self::Shuffle(result) => result.as_ref().as_ref().ok().map(|value| &value.next),
        }
    }
    fn view(&self, seats: &[SeatId]) -> Value {
        match self {
            Self::Move(result) => match result.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"outcome":projection::zone_move_outcome(&value.outcome)})
                }
                Err(error) => projection::object_identity_error(error),
            },
            Self::Change(result) => match result.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"before":value.before.as_ref().map(projection::object),"after":projection::object(&value.after)})
                }
                Err(error) => projection::object_identity_error(error),
            },
            Self::Registry(result) => match result.as_ref() {
                Ok(value) => json!({"tag":"Ok","state":projection::card_objects(value,seats)}),
                Err(error) => projection::object_identity_error(error),
            },
            Self::Status(result) => match result.as_ref() {
                Ok((value, changes)) => {
                    json!({"tag":"Ok","state":projection::card_objects(value,seats),"changes":changes.iter().map(|value|json!({"before":projection::object(&value.before),"after":projection::object(&value.after)})).collect::<Vec<_>>()})
                }
                Err(error) => projection::object_identity_error(error),
            },
            Self::Shuffle(result) => match result.as_ref() {
                Ok(value) => {
                    json!({"tag":"Ok","state":projection::card_objects(&value.next,seats),"shuffle":shuffle(&value.record),"entropy":{"position":value.entropy.position(),"remaining":value.entropy.remaining()}})
                }
                Err(error) => projection::object_identity_error(error),
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct RegistryWitness {
    initialization: Box<Result<EffectWindow, EffectError>>,
    attempts: Vec<(GameObjects, RegistryResult, bool)>,
    final_registry: Option<GameObjects>,
    opening: Option<OpeningWitness>,
}

#[derive(Debug, PartialEq, Eq)]
struct OpeningWitness {
    initialization: Box<Result<EffectWindow, EffectError>>,
    begin: Option<Box<Result<EffectWindow, EffectError>>>,
    shuffle: Option<Box<Result<OpeningShuffleTransition, EffectError>>>,
    final_state: Option<Box<EffectWindow>>,
}
type OpeningShuffleTransition =
    crate::occurrence::RulesTransition<EffectWindow, (EntropyTape, ShuffleRecord)>;

fn shuffle(record: &ShuffleRecord) -> Value {
    json!({"player":record.player.0,"algorithm":record.algorithm,"before":record.before.iter().map(projection::opaque).collect::<Vec<_>>(),"after":record.after.iter().map(projection::opaque).collect::<Vec<_>>(),"entropy_start":record.entropy_start,"entropy_end":record.entropy_end})
}

fn entropy_words(value: &Value) -> Result<Vec<u64>, String> {
    value
        .as_array()
        .ok_or("missing literal entropy words")?
        .iter()
        .map(|word| word.as_u64().map(Ok).unwrap_or_else(|| integer(word)))
        .collect::<Result<_, _>>()
}

fn physical_alias(
    fixture: &Value,
    alias: &str,
    registry: &GameObjects,
) -> Result<ObjectId, String> {
    let cards = rows(&fixture["seed"], "cards")?;
    let found = cards
        .iter()
        .filter(|card| card["alias"] == alias)
        .collect::<Vec<_>>();
    let [card] = found.as_slice() else {
        return Err("nonunique registry card alias".into());
    };
    registry
        .current_identity(CardId(
            card["card_id"].as_u64().ok_or("invalid physical card ID")?,
        ))
        .ok_or("unknown current alias incarnation".into())
}

fn operation(
    source: &GameObjects,
    action: &Value,
    joins: &Joins,
    fixture: &Value,
    seeded: &EffectWindow,
    adopt: bool,
) -> Result<RegistryResult, String> {
    if string(action, "op")? != "trusted_call" || string(action, "owner")? != "GameObjects" {
        return Err("unsupported primitive registry operation".into());
    }
    let object = |value: &Value| joins.object(value, fixture, seeded);
    let expected = source.revision();
    let seats = seeded.window().participants();
    Ok(match string(action, "method")? {
        "apply_zone_move" => {
            if action
                .get("semantic_cause")
                .is_some_and(|value| !value.is_null())
            {
                return Err("primitive move cannot invent a semantic publisher cause".into());
            }
            let destination = match string(action, "destination")? {
                "Graveyard" => ObjectDestination::Graveyard,
                "Hand" => ObjectDestination::Hand,
                _ => return Err("unsupported primitive destination".into()),
            };
            let object = object(&action["card"])?;
            RegistryResult::Move(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::ApplyZoneMove {
                    expected,
                    object,
                    destination,
                },
                |source| source.apply_zone_move(expected, object, destination),
                |result| Ok(RegistryResult::Move(Box::new(result.clone())).view(seats)),
                |value| Some(&value.next),
                adopt,
            )?))
        }
        "set_tapped" => {
            let object = object(&action["object"])?;
            let tapped = action["tapped"].as_bool().ok_or("missing tapped bool")?;
            RegistryResult::Change(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::SetTapped {
                    expected,
                    object,
                    tapped,
                },
                |source| source.set_tapped(expected, object, tapped),
                |result| Ok(RegistryResult::Change(Box::new(result.clone())).view(seats)),
                |value| Some(&value.next),
                adopt,
            )?))
        }
        "set_tapped_batch" => {
            let objects = rows(action, "objects")?
                .iter()
                .map(object)
                .collect::<Result<Vec<_>, _>>()?;
            let tapped = action["tapped"].as_bool().ok_or("missing tapped bool")?;
            RegistryResult::Status(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::SetTappedBatch {
                    expected,
                    objects: &objects,
                    tapped,
                },
                |source| source.set_tapped_batch(expected, &objects, tapped),
                |result| Ok(RegistryResult::Status(Box::new(result.clone())).view(seats)),
                |value| Some(&value.0),
                adopt,
            )?))
        }
        "apply_controller" => {
            let object = object(&action["object"])?;
            let controller = packet::seat(string(action, "controller")?)?;
            RegistryResult::Change(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::ApplyController {
                    expected,
                    object,
                    controller,
                },
                |source| source.apply_controller(expected, object, controller),
                |result| Ok(RegistryResult::Change(Box::new(result.clone())).view(seats)),
                |value| Some(&value.next),
                adopt,
            )?))
        }
        "record_turn_start" => {
            let player = packet::seat(string(action, "controller")?)?;
            RegistryResult::Registry(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::RecordTurnStart { expected, player },
                |source| source.record_turn_start(expected, player),
                |result| Ok(RegistryResult::Registry(Box::new(result.clone())).view(seats)),
                |value| Some(value),
                adopt,
            )?))
        }
        "reorder_library" => {
            let owner = packet::seat(string(action, "player")?)?;
            let order = rows(action, "order_aliases")?
                .iter()
                .map(|alias| {
                    physical_alias(
                        fixture,
                        alias.as_str().ok_or("invalid order alias")?,
                        source,
                    )
                })
                .collect::<Result<Vec<_>, String>>()?;
            RegistryResult::Registry(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::ReorderLibrary {
                    expected,
                    owner,
                    order: &order,
                },
                |source| source.reorder_library(expected, owner, order.clone()),
                |result| Ok(RegistryResult::Registry(Box::new(result.clone())).view(seats)),
                |value| Some(value),
                adopt,
            )?))
        }
        "shuffle_library" => {
            let owner = packet::seat(string(action, "player")?)?;
            let words = entropy_words(&action["entropy_words"])?;
            let entropy = EntropyTape::new(words.clone());
            RegistryResult::Shuffle(Box::new(capture::card(
                source,
                seats,
                projection::GameObjectsCall::ShuffleLibrary {
                    expected,
                    owner,
                    entropy: &entropy,
                    entropy_words: &words,
                },
                |source| source.shuffle_library(expected, owner, &entropy),
                |result| Ok(RegistryResult::Shuffle(Box::new(result.clone())).view(seats)),
                |value| Some(&value.next),
                adopt,
            )?))
        }
        _ => return Err("unsupported primitive registry method".into()),
    })
}

pub(super) fn run(selected: &packet::SelectedInput) -> Result<(Value, V2Witness), String> {
    let prepared = selected.prepare_seed(
        packet::SeedMode::TrustedUnit,
        packet::DefinitionPolicy::ValidateSelected,
    )?;
    if !prepared.trusted_precheck_overrides.is_empty()
        || prepared.initial_status.iter().any(|(_, value)| *value)
        || !matches!(prepared.initial, packet::Initial::TrustedUnitPrecheck {})
    {
        return Err("registry recipe cannot hide private preparation".into());
    }
    let initialization =
        crate::test_support::native_calls::effect_initialize(prepared.seed.clone());
    let mut witness = RegistryWitness {
        initialization: Box::new(initialization.clone()),
        attempts: vec![],
        final_registry: None,
        opening: None,
    };
    let seeded = match initialization {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","initialization":projection::effect_error(&error)}),
                V2Witness::Registry(Box::new(witness)),
            ));
        }
    };
    let joins = Joins::seed(&selected.fixture, &seeded)?;
    let mut registry = seeded.window().cards().clone();
    let seats = seeded.window().participants();
    let actions = rows(&selected.fixture, "actions")?;
    if selected.fixture["stop"]["after_action_ordinal"].as_u64() != Some(actions.len() as u64)
        || selected.fixture["stop"]["no_extra_pass"] != true
    {
        return Err("unsupported primitive finite stop".into());
    }
    let mut records = Vec::new();
    let mut failed = None;
    for (index, action) in actions.iter().enumerate() {
        if action["op"] == "separate_effect_window_opening_branch" {
            let (view, branch) =
                opening_branch(selected, &prepared.seed, &prepared.aliases, action)?;
            let complete = view["status"] == "completed_recipe";
            records.push(json!({"ordinal":index+1,"operation":action,"independent_branch":view}));
            witness.opening = Some(branch);
            if !complete {
                failed = Some(index + 1);
                break;
            }
            continue;
        }
        let source = registry.clone();
        let adopt = action["adopt"].as_bool().unwrap_or(true);
        let result = operation(&source, action, &joins, &selected.fixture, &seeded, adopt)?;
        let actual = result.view(seats);
        let next = result.successor().cloned();
        records.push(capture::card_point(json!({"ordinal":index+1,"operation":action,"source_state":projection::card_objects(&source,seats),"result":actual,"adopt_requested":adopt,"retained_source_eq":source==registry,"canonical_rules_append":null,"primitive_has_no_publisher":true}),&source,seats,"registry actual source",Some(&format!("/observation/records/{index}/source_state")))?);
        witness.attempts.push((source, result, adopt));
        if adopt {
            if let Some(next) = next {
                registry = next;
            } else {
                failed = Some(index + 1);
                break;
            }
        }
    }
    witness.final_registry = Some(registry.clone());
    Ok((
        capture::card_point(
            json!({"status":if failed.is_none(){"completed_recipe"}else{"failed"},"scope":"trusted_registry","initialization":{"tag":"Ok","state":projection::effect(&seeded,&prepared.aliases)?},"records":records,"failed_action_ordinal":failed,"stop_reached":failed.is_none(),"final_registry":projection::card_objects(&registry,seats),"executor_registry_not_replaced":true,"stop":selected.fixture["stop"]}),
            &registry,
            seats,
            "registry actual final owner",
            Some("/observation/final_registry"),
        )?,
        V2Witness::Registry(Box::new(witness)),
    ))
}

fn opening_branch(
    selected: &packet::SelectedInput,
    seed: &GameSeed,
    aliases: &packet::AliasContext,
    action: &Value,
) -> Result<(Value, OpeningWitness), String> {
    if action["source_snapshot"] != "fresh_original_fixture_seed_before_any_primitive_mutation" {
        return Err("opening branch must use exact original seed".into());
    }
    let [begin, shuffle_action] = rows(action, "sequence")? else {
        return Err("opening branch requires exactly begin and shuffle".into());
    };
    if begin["op"] != "trusted_call"
        || begin["owner"] != "EffectWindow"
        || begin["method"] != "begin_opening"
        || shuffle_action["op"] != "trusted_call"
        || shuffle_action["owner"] != "EffectWindow"
        || shuffle_action["method"] != "opening_shuffle"
    {
        return Err("unsupported independent opening calls".into());
    }
    let initialization = crate::test_support::native_calls::effect_initialize(seed.clone());
    let mut witness = OpeningWitness {
        initialization: Box::new(initialization.clone()),
        begin: None,
        shuffle: None,
        final_state: None,
    };
    let source = match initialization {
        Ok(value) => value,
        Err(error) => {
            return Ok((
                json!({"status":"failed","initialization":projection::effect_error(&error)}),
                witness,
            ));
        }
    };
    let expected = source.revision();
    let begin_result = capture::state(
        &source,
        aliases,
        projection::EffectCall::BeginOpening { expected },
        |source| source.begin_opening(expected),
        true,
    )?;
    let begin_view = match &begin_result {
        Ok(value) => json!({"tag":"Ok","state":projection::effect(value,aliases)?}),
        Err(error) => projection::effect_error(error),
    };
    witness.begin = Some(Box::new(begin_result.clone()));
    let state = match begin_result {
        Ok(value) => value,
        Err(_) => {
            witness.final_state = Some(Box::new(source));
            return Ok((
                json!({"status":"failed","begin_result":begin_view}),
                witness,
            ));
        }
    };
    let words = entropy_words(&action["entropy_words"])?;
    let tape = EntropyTape::new(words.clone());
    let expected = state.revision();
    let player = packet::seat(string(shuffle_action, "player")?)?;
    let adopt = shuffle_action["adopt"].as_bool().unwrap_or(true);
    let result = capture::transition(
        &state,
        aliases,
        projection::EffectCall::OpeningShuffle {
            expected,
            player,
            entropy: &tape,
            entropy_words: &words,
        },
        |source| source.opening_shuffle(expected, player, &tape),
        |value| json!({"entropy":{"position":value.0.position(),"remaining":value.0.remaining()},"shuffle":shuffle(&value.1)}),
        adopt,
    )?;
    let view = match &result {
        Ok(value) => {
            json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"shuffle":shuffle(&value.result().1),"entropy":{"position":value.result().0.position(),"remaining":value.result().0.remaining()},"append":projection::journal(value.append())})
        }
        Err(error) => projection::effect_error(error),
    };
    let complete = !adopt || result.is_ok();
    witness.final_state = Some(Box::new(if adopt {
        result
            .as_ref()
            .map(|value| value.state().clone())
            .unwrap_or(state)
    } else {
        state
    }));
    witness.shuffle = Some(Box::new(result));
    Ok((
        capture::effect_point(
            json!({"status":if complete{"completed_recipe"}else{"failed"},"scope":"independent_private_executor_branch_not_root_pause_or_primitive_registry_substitution","initial_state":projection::effect(&source,aliases)?,"begin_result":begin_view,"shuffle_result":view,"adopt_requested":adopt,"source_unchanged":source==*witness.initialization.as_ref().as_ref().unwrap(),"seed_input":selected.fixture["seed"]}),
            &source,
            aliases,
            "registry independent opening source",
        )?,
        witness,
    ))
}
