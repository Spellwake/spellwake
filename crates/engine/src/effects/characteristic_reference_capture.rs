//! Opt-in finite capture of existing common, static, resolved and control-history recipes.
//! This recorder observes typed owners once and delegates all value projection to
//! the existing test-support serializers. It does not replay or adopt a prefix.
use super::characteristic_observations::{self, Observation};
use crate::{
    characteristics::{CharacteristicState, EvaluatedCharacteristics, ObjectCharacteristics},
    game::{GameContext, GameError, GameId, GameInput, GameState, NativeCandidate},
    object_identity::{CardId, GameObjects, ObjectId, ObjectZone},
    priority::SeatId,
    program::Program,
    setup::GameSeed,
    test_support::{
        characteristic_trajectories::{self as routes, TrajectoryObservation},
        current_capture,
        packet::AliasContext,
        program, projection,
    },
    turn::TurnStart,
};
use serde_json::{Value, json};
use spellwake_contracts::creature_types::CreatureTypeVocabulary;
use std::{collections::BTreeMap, env, fs, io::Write, path::Path, sync::Arc};

const CODEC: &str = "spellwake.characteristics-native-common-values.v1";
const STATIC_CODEC: &str = "spellwake.characteristics-native-static-values.v1";
const RESOLVED_CODEC: &str = "spellwake.characteristics-native-resolved-values.v1";
const CONTROL_HISTORY_CODEC: &str = "spellwake.characteristics-native-control-history-values.v1";
const CONTROL_HISTORY_CASES: &[&str] = &["CL-TAP-NEW-MANA", "CL-TAP-NEW-PUMP"];
const COMMON_CASES: &[&str] = &[
    "CL-TAP-OLD-MANA",
    "CL-TAP-OLD-PUMP",
    "CL-TYPE-RESTORE",
    "CL-DAMAGE-SHIELD",
    "CL-DAMAGE-MULTIPLIER",
    "CL-CLEANUP-SIMULTANEOUS",
    "CL-CLEANUP-NONCREATURE-MARKS",
];
const RESOLVED_CASES: &[&str] = &[
    "CL-TYPE-EXPLICIT-REMOVE",
    "CL-TYPE-REPLACE",
    "CL-PT-ZERO",
    "CL-PT-MULTIPART",
    "CL-PT-CORE",
    "CL-TARGET-LATER-BOUNCE",
];
const STATIC_CASES: &[&str] = &[
    "CL-STARTED-NONEMPTY",
    "CL-STARTED-EMPTY",
    "CL-STARTED-ABSENT",
    "CL-STATIC-COUNT-CONTROLLER",
    "CL-DEP-SELECTOR-AB",
    "CL-DEP-SELECTOR-BA",
    "CL-DEP-SUCCESS-NOT-INTENT-GC",
    "CL-DEP-SUCCESS-NOT-INTENT-CG",
    "CL-DEP-ADDITIVE-NOT-TOTALS",
    "CL-DEP-RECONSIDER-AB",
    "CL-DEP-RECONSIDER-BA",
    "CL-LOOP-EARLY-OUTSIDE-AB",
    "CL-LOOP-EARLY-OUTSIDE-BA",
    "CL-LOOP-LATE-OUTSIDE-AB",
    "CL-LOOP-LATE-OUTSIDE-BA",
    "CL-DEP-DISAPPEARING-CAB",
    "CL-DEP-DISAPPEARING-BAC",
];

#[derive(Clone, Copy)]
enum CaptureFamily {
    Common,
    Static,
    Resolved,
    ControlHistory,
}

impl CaptureFamily {
    fn codec(self) -> &'static str {
        match self {
            Self::Common => CODEC,
            Self::Static => STATIC_CODEC,
            Self::Resolved => RESOLVED_CODEC,
            Self::ControlHistory => CONTROL_HISTORY_CODEC,
        }
    }

    fn cases(self) -> &'static [&'static str] {
        match self {
            Self::Common => COMMON_CASES,
            Self::Static => STATIC_CASES,
            Self::Resolved => RESOLVED_CASES,
            Self::ControlHistory => CONTROL_HISTORY_CASES,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum TrajectoryRecord {
    InitializeInput {
        game: GameId,
        context: Box<GameContext>,
        seed: Box<GameSeed>,
        start: TurnStart,
    },
    AdvanceInput {
        source: Box<GameState>,
        input: Box<GameInput>,
    },
    Returned {
        source: Option<Box<GameState>>,
        result: Box<NativeCandidate>,
    },
    Evaluated {
        label: &'static str,
        state: Box<GameState>,
        result: Box<EvaluatedCharacteristics>,
    },
    AdvanceAttemptInput {
        source: Box<GameState>,
        input: Box<GameInput>,
    },
    AdvanceAttemptResult {
        source: Box<GameState>,
        result: Box<Result<NativeCandidate, GameError>>,
    },
    CurrentInput {
        source: Box<GameState>,
        object: ObjectId,
    },
    Current {
        state: Box<GameState>,
        result: Box<ObjectCharacteristics>,
    },
    Checkpoint {
        label: &'static str,
        state: Box<GameState>,
    },
}

fn trajectory(value: TrajectoryObservation<'_>) {
    characteristic_observations::trajectory_record(match value {
        TrajectoryObservation::BeforeInitialize {
            game,
            context,
            seed,
            start,
        } => TrajectoryRecord::InitializeInput {
            game,
            context: Box::new(context.clone()),
            seed: Box::new(seed.clone()),
            start,
        },
        TrajectoryObservation::BeforeAdvance { source, input } => TrajectoryRecord::AdvanceInput {
            source: Box::new(source.clone()),
            input: Box::new(input.clone()),
        },
        TrajectoryObservation::Initialize { result, .. } => TrajectoryRecord::Returned {
            source: None,
            result: Box::new(result.clone()),
        },
        TrajectoryObservation::Advance { source, result, .. } => TrajectoryRecord::Returned {
            source: Some(Box::new(source.clone())),
            result: Box::new(result.clone()),
        },
        TrajectoryObservation::Evaluated {
            label,
            state,
            result,
        } => TrajectoryRecord::Evaluated {
            label,
            state: Box::new(state.clone()),
            result: Box::new(result.clone()),
        },
        TrajectoryObservation::Checkpoint { label, state } => TrajectoryRecord::Checkpoint {
            label,
            state: Box::new(state.clone()),
        },
        // Existing common/static codecs retain their original event vocabulary.
        TrajectoryObservation::BeforeCurrent { .. }
        | TrajectoryObservation::Current { .. }
        | TrajectoryObservation::BeforeAdvanceAttempt { .. }
        | TrajectoryObservation::AdvanceAttemptResult { .. } => {
            return;
        }
    });
}

fn resolved_trajectory(value: TrajectoryObservation<'_>) {
    match value {
        TrajectoryObservation::BeforeCurrent { state, object } => {
            characteristic_observations::trajectory_record(TrajectoryRecord::CurrentInput {
                source: Box::new(state.clone()),
                object,
            });
        }
        TrajectoryObservation::Current { state, result } => {
            characteristic_observations::trajectory_record(TrajectoryRecord::Current {
                state: Box::new(state.clone()),
                result: Box::new(result.clone()),
            });
        }
        other => trajectory(other),
    }
}

fn control_history_trajectory(value: TrajectoryObservation<'_>) {
    match value {
        TrajectoryObservation::BeforeAdvanceAttempt { source, input } => {
            characteristic_observations::trajectory_record(TrajectoryRecord::AdvanceAttemptInput {
                source: Box::new(source.clone()),
                input: Box::new(input.clone()),
            });
        }
        TrajectoryObservation::AdvanceAttemptResult { source, result } => {
            characteristic_observations::trajectory_record(
                TrajectoryRecord::AdvanceAttemptResult {
                    source: Box::new(source.clone()),
                    result: Box::new(result.clone()),
                },
            );
        }
        other => resolved_trajectory(other),
    }
}

/// This is a finite dispatcher over the existing routes, not a fixture decoder.
fn route(case: &str) {
    match case {
        "CL-TAP-NEW-MANA" => routes::new_tap(true),
        "CL-TAP-NEW-PUMP" => routes::new_tap(false),
        "CL-TAP-OLD-MANA" => routes::tap_old(true),
        "CL-TAP-OLD-PUMP" => routes::tap_old(false),
        "CL-TYPE-RESTORE" => routes::type_restore(),
        "CL-DAMAGE-SHIELD" => routes::latent_damage(true, false, true),
        "CL-DAMAGE-MULTIPLIER" => routes::latent_damage(false, true, false),
        "CL-CLEANUP-SIMULTANEOUS" => routes::cleanup_simultaneous(),
        "CL-CLEANUP-NONCREATURE-MARKS" => routes::cleanup_noncreature_marks(),
        "CL-TYPE-EXPLICIT-REMOVE" => routes::type_explicit_remove(),
        "CL-TYPE-REPLACE" => routes::type_replace(),
        "CL-PT-ZERO" => routes::pt_zero(),
        "CL-PT-MULTIPART" => routes::pt_multipart(),
        "CL-PT-CORE" => routes::pt_core(),
        "CL-TARGET-LATER-BOUNCE" => routes::target_later_bounce(),
        "CL-STARTED-NONEMPTY" => routes::started_nonempty(),
        "CL-STATIC-COUNT-CONTROLLER" => routes::static_count_controller(),
        "CL-DEP-SUCCESS-NOT-INTENT-GC" => routes::success_not_intent(true),
        "CL-DEP-SUCCESS-NOT-INTENT-CG" => routes::success_not_intent(false),
        "CL-DEP-ADDITIVE-NOT-TOTALS" => routes::additive_not_totals(),
        "CL-DEP-DISAPPEARING-CAB" => routes::disappearing(true),
        "CL-DEP-DISAPPEARING-BAC" => routes::disappearing(false),
        other if STATIC_CASES.contains(&other) => {
            crate::characteristics::frozen_tests::capture_static_case(other)
        }
        _ => panic!("case is outside the finite source-reviewed native recipes"),
    }
}

fn aliases(case: &str) -> Result<(AliasContext, Value), String> {
    let bindings: Vec<(u64, &str)> = match case {
        "CL-TAP-NEW-MANA" | "CL-TAP-NEW-PUMP" => vec![
            (1, "S"),
            (2, "R"),
            (3, "Animate1"),
            (4, "Animate2"),
            (5, "P0Draw"),
            (6, "P1Draw"),
        ],
        "CL-TAP-OLD-MANA" | "CL-TAP-OLD-PUMP" => vec![(1, "S"), (2, "R"), (3, "Animate1")],
        "CL-TYPE-RESTORE" => vec![(1, "X"), (2, "RemoveCreature"), (3, "AddCreature")],
        "CL-TYPE-EXPLICIT-REMOVE" => vec![(1, "X"), (2, "RemoveElf"), (3, "TypeOnly")],
        "CL-TYPE-REPLACE" => vec![(1, "X"), (2, "Replace"), (3, "TypeOnly")],
        "CL-PT-ZERO" => vec![(1, "X"), (2, "Animate")],
        "CL-PT-MULTIPART" => vec![(1, "X"), (2, "AnimateSet")],
        "CL-PT-CORE" => vec![
            (1, "X"),
            (2, "Set"),
            (3, "Modify"),
            (4, "Counter"),
            (5, "Switch"),
        ],
        "CL-TARGET-LATER-BOUNCE" => vec![(1, "X"), (2, "Body")],
        "CL-DAMAGE-SHIELD" | "CL-DAMAGE-MULTIPLIER" => vec![(1, "X"), (2, "Body")],
        "CL-CLEANUP-SIMULTANEOUS" => vec![
            (1, "X"),
            (2, "D1"),
            (3, "D2"),
            (4, "D3"),
            (5, "D4"),
            (6, "D5"),
            (7, "D6"),
            (8, "D7"),
            (9, "D8"),
            (10, "Boost"),
            (11, "TempType"),
            (12, "Damage"),
        ],
        "CL-CLEANUP-NONCREATURE-MARKS" => vec![(1, "X"), (2, "Damage"), (3, "Lose"), (4, "Latent")],
        "CL-STARTED-NONEMPTY" => vec![(1, "A"), (2, "X"), (3, "Y")],
        "CL-STARTED-EMPTY" | "CL-STARTED-ABSENT" => vec![(1, "A"), (2, "X")],
        "CL-STATIC-COUNT-CONTROLLER" => {
            vec![(1, "S"), (2, "X"), (3, "Y"), (4, "Z"), (5, "YtoGoblin")]
        }
        "CL-DEP-SELECTOR-AB" | "CL-DEP-SELECTOR-BA" => vec![(1, "A"), (2, "B"), (3, "R"), (4, "T")],
        "CL-DEP-RECONSIDER-AB" | "CL-DEP-RECONSIDER-BA" => {
            vec![(1, "A"), (2, "B"), (3, "C"), (4, "R"), (5, "T")]
        }
        "CL-LOOP-EARLY-OUTSIDE-AB"
        | "CL-LOOP-EARLY-OUTSIDE-BA"
        | "CL-LOOP-LATE-OUTSIDE-AB"
        | "CL-LOOP-LATE-OUTSIDE-BA" => {
            vec![(1, "A"), (2, "B"), (3, "C"), (4, "X"), (5, "Y"), (6, "Z")]
        }
        "CL-DEP-SUCCESS-NOT-INTENT-GC" => vec![(1, "X"), (2, "G"), (3, "C")],
        "CL-DEP-SUCCESS-NOT-INTENT-CG" => vec![(1, "X"), (2, "C"), (3, "G")],
        "CL-DEP-ADDITIVE-NOT-TOTALS" => vec![(1, "X"), (2, "M1"), (3, "M2")],
        "CL-DEP-DISAPPEARING-CAB" => vec![(1, "X"), (2, "A"), (3, "C"), (4, "B")],
        "CL-DEP-DISAPPEARING-BAC" => vec![(1, "X"), (2, "A"), (3, "B"), (4, "C")],
        _ => return Err("unknown native comparison case".into()),
    };
    let pairs = bindings
        .iter()
        .map(|(card, alias)| (CardId(*card), (*alias).to_owned()))
        .collect::<Vec<_>>();
    let value = json!(
        bindings
            .iter()
            .map(|(card, alias)| json!({"card_id":card,"alias":alias}))
            .collect::<Vec<_>>()
    );
    Ok((AliasContext::from_card_aliases(&pairs)?, value))
}

/// These values are copies of the existing projection of an actual typed owner.
/// No expected assertion operand or reconstructed registry supplies a value.
fn common_values(execution: &super::EffectWindow, state: &Value) -> Value {
    json!({
        "cards":state["cards"], "mana":state["mana"], "life":state["life"],
        "continuous_effects":state["continuous_effects"],
        "prevention_shields":state["prevention_shields"],
        "damage_multipliers":state["damage_multipliers"],
        "hand_counts":execution.window().participants().iter().map(|seat| json!({
            "player":seat.0,
            "count":execution.window().cards().iter().filter(|card| card.zone() == crate::object_identity::ObjectZone::Hand(*seat)).count(),
        })).collect::<Vec<_>>(),
        "library_counts":execution.window().participants().iter().map(|seat| json!({
            "player":seat.0, "count":execution.window().cards().library(*seat).len(),
        })).collect::<Vec<_>>(),
    })
}

/// Owned copies from the original private fixture and its existing query result.
/// There is no GameState shell, canonical write, replay or rules evaluation here.
#[derive(Debug, Clone)]
pub(super) struct PureEvaluationSnapshot {
    cards: GameObjects,
    programs: BTreeMap<CardId, Program>,
    state: CharacteristicState,
    vocabulary: Arc<CreatureTypeVocabulary>,
    name_rules: spellwake_contracts::names::NameRules,
    objects: BTreeMap<&'static str, ObjectId>,
    result: EvaluatedCharacteristics,
    source_unchanged: bool,
}

pub(crate) fn observe_pure_fixture(
    world: crate::characteristics::CharacteristicWorld<'_>,
    objects: &BTreeMap<&'static str, ObjectId>,
    result: &EvaluatedCharacteristics,
    source_unchanged: bool,
) {
    characteristic_observations::pure_evaluation(|| PureEvaluationSnapshot {
        cards: world.cards.clone(),
        programs: world.programs.clone(),
        state: world.state.clone(),
        vocabulary: Arc::new(world.vocabulary.clone()),
        name_rules: world.name_rules.clone(),
        objects: objects.clone(),
        result: result.clone(),
        source_unchanged,
    });
}

fn actual_evaluation(result: &EvaluatedCharacteristics) -> Value {
    json!({
        "objects":result.objects.values().map(projection::current_characteristics).collect::<Vec<_>>(),
        "trace":projection::evaluation_trace(&result.trace),
        "trace_classification":"actual-native-query-trace; not a reference application-order observation",
    })
}

fn pure_evaluation(
    value: &PureEvaluationSnapshot,
    aliases: &AliasContext,
) -> Result<Value, String> {
    for (alias, object) in &value.objects {
        let physical = value
            .cards
            .get(*object)
            .ok_or("pure alias is not an actual fixture object")?;
        if physical.card_id().and_then(|id| aliases.alias(id)) != Some(*alias) {
            return Err(
                "actual pure fixture alias/CardId mapping differs from selected source".into(),
            );
        }
    }
    if value.objects.len() != value.cards.iter().count()
        || value.result.objects.len() != value.objects.len()
    {
        return Err("actual pure fixture/result membership is not complete".into());
    }
    let owners = value
        .cards
        .iter()
        .map(|card| card.owner())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let cards = value.cards.iter().map(|physical| {
        let current = value.result.get(physical.id()).ok_or("actual query omitted a fixture card")?;
        let definition = physical.card_id().and_then(|id|value.programs.get(&id)).ok_or("actual fixture omitted a Program")?;
        Ok(json!({
            "physical":projection::object(physical), "alias":physical.card_id().and_then(|id|aliases.alias(id)),
            "program":program::view(definition), "characteristics":projection::current_characteristics(current),
            "battlefield_counters":(physical.zone()==ObjectZone::Battlefield).then(||program::counters(&value.state.counters(physical.id()))),
            "battlefield_marked_damage":{"status":"unavailable","reason":"pure CharacteristicWorld fixture has no marked-damage owner"},
            "latent_after_setting_base":match &current.power_toughness {
                Some(pt)=>json!({"status":"observed","value":program::pt(&pt.base),"source":"actual power_toughness.base"}),
                None=>json!({"status":"unavailable","reason":"no stored numeric latent PT for a noncreature; actual generated/applied SetPT intent remains separately in the native trace"}),
            },
        }))
    }).collect::<Result<Vec<_>, String>>()?;
    Ok(json!({
        "tag":"PureCharacteristicEvaluation", "label":"final", "boundary":"trusted-private-fixture-query",
        "borrowed_source_full_fixture_eq":value.source_unchanged,
        "fixture_input":{
            "cards":projection::card_objects(&value.cards,&owners),
            "programs":value.programs.iter().map(|(card,p)|json!({"card_id":card.0,"program":program::view(p)})).collect::<Vec<_>>(),
            "characteristic_timestamps":value.state.object_timestamps().map(|(object,timestamp)|json!({"object":projection::opaque(object),"timestamp":timestamp.sequence().to_string()})).collect::<Vec<_>>(),
            "continuous_effects":value.state.effects().map(projection::continuous_effect).collect::<Vec<_>>(),
            "characteristic_resources":{"next_effect":value.state.next_effect_sequence().to_string(),"next_timestamp":value.state.next_timestamp_sequence().to_string()},
            "vocabulary":crate::test_support::creature_vocabulary::view(&value.vocabulary),
            "name_rules":value.name_rules.groups(),"name_rules_id":value.name_rules.id().to_string(),
        },
        "cards":cards,"actual_evaluation":actual_evaluation(&value.result),
        "unavailable_owners":["GameState basis/scheduler/accepted candidate/journals","mana/life/marked damage"],
        "comparison_class":"candidate-common-fields-only; exact fixture/result/field join required",
    }))
}

fn operation(value: &crate::program::Instruction) -> Value {
    // Reuse the sole instruction serializer through a one-instruction view.
    program::body(&crate::program::ExecutableBody::new(
        crate::program::BodyTarget::None,
        vec![value.clone()],
    ))["instructions"][0]
        .clone()
}

fn internal(value: &Observation, aliases: &AliasContext) -> Result<Value, String> {
    Ok(match value {
        Observation::PureEvaluation(value) => pure_evaluation(value, aliases)?,
        Observation::AcceptedState {
            failed_draws,
            losses,
        } => json!({
            "tag":"PrivateAcceptedMemory", "failed_draws":failed_draws.iter().map(|v|v.0).collect::<Vec<_>>(),
            "state_based_losses":losses.iter().map(|v|v.0).collect::<Vec<_>>(),
        }),
        Observation::Published(journal) => {
            json!({"tag":"PublishedJournal","journal":projection::journal(journal)})
        }
        Observation::PublishedState { execution, journal } => {
            let state = projection::effect(execution, aliases)?;
            json!({"tag":"CanonicalPublished","execution":state,"common_values":common_values(execution,&state),"journal":projection::journal(journal),"cut":"actual publisher after trigger-state adoption and before local append","comparison_class":"native-diagnostic-unless-explicit-common-cut-binding","root_adoption":"not implied; bind enclosing ReturnedCandidate"})
        }
        Observation::Instruction {
            phase,
            entry,
            instruction,
            operation: value,
            execution,
        } => {
            let state = projection::effect(execution, aliases)?;
            json!({"tag":"OperationalInstruction","phase":phase,"entry":projection::opaque(entry),"instruction":instruction,"operation":value.as_ref().map(operation),"execution":state,"common_values":common_values(execution,&state),"comparison_class":"native-diagnostic-unless-explicit-common-cut-binding"})
        }
        Observation::ChecksObserved {
            boundary,
            event,
            cards,
            actions_empty,
            execution,
        } => {
            let state = projection::effect(execution, aliases)?;
            json!({"tag":"SharedChecksObserved","boundary":crate::test_support::check_trace::check_boundary_view(*boundary),"event":crate::test_support::check_trace::check_event_view(*event),"cards":projection::card_objects(cards,execution.window().participants()),"actions_empty":actions_empty,"execution":state,"common_values":common_values(execution,&state),"comparison_class":"native-diagnostic"})
        }
        Observation::Trajectory(_) => {
            return Err("trajectory record passed to internal-cut serializer".into());
        }
        Observation::DrawRequested { .. }
        | Observation::Drawn { .. }
        | Observation::DrawCompleted { .. } => {
            return Err(
                "selected finite routes unexpectedly requested a draw; no unreviewed record codec"
                    .into(),
            );
        }
    })
}

/// Actual collection membership/order only; this creates no zone-order owner.
fn control_history_collections(state: &GameState, aliases: &AliasContext) -> Value {
    let window = state.execution().window();
    let cards = window.cards();
    json!({
        "players":window.participants().iter().map(|seat| json!({
            "player":seat.0,
            "hand":cards.iter().filter(|card|card.zone()==ObjectZone::Hand(*seat)).map(|card|json!({
                "physical":projection::object(card),"alias":card.card_id().and_then(|id|aliases.alias(id)),
            })).collect::<Vec<_>>(),
            "library":cards.library(*seat).iter().map(|object| {
                let card = cards.get(*object).expect("actual ordered Library member must exist");
                json!({"physical":projection::object(card),"alias":card.card_id().and_then(|id|aliases.alias(id))})
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "hand_order":"actual canonical GameObjects::iter order; no semantic Hand-order owner",
        "library_order":"actual GameObjects::library top-to-bottom slice",
        "comparison_class":"complete native collections; original input/movement/returned-owner identity join required",
    })
}

/// Reuse the scoped real draw cuts only for this finite control-history family.
fn control_history_internal(
    value: &Observation,
    aliases: &AliasContext,
    participants: &[SeatId],
) -> Result<Value, String> {
    if participants.is_empty() {
        return Err(
            "control-history internal cut without actual initialization participants".into(),
        );
    }
    Ok(match value {
        Observation::DrawRequested {
            player,
            origin,
            amount,
            cards,
            ..
        } => json!({
            "tag":"DrawRequested","player":player.0,"origin":projection::draw_origin(origin),
            "amount":amount,"cards":projection::card_objects(cards,participants),
            "comparison_class":"actual requested draw cut; bind exact returned candidate and Turn Draw journal",
        }),
        Observation::Drawn {
            player,
            origin,
            before,
            after,
            movement,
            ..
        } => json!({
            "tag":"Drawn","player":player.0,"origin":projection::draw_origin(origin),
            "before":projection::card_objects(before,participants),"after":projection::card_objects(after,participants),
            "movement":{"before":projection::object(&movement.before),"after":projection::object(&movement.after)},
            "comparison_class":"actual physical draw transaction; no recorder adoption",
        }),
        Observation::DrawCompleted {
            player,
            origin,
            journal,
            cards,
            ..
        } => json!({
            "tag":"DrawCompleted","player":player.0,"origin":projection::draw_origin(origin),
            "journal":projection::draws(journal),"cards":projection::card_objects(cards,participants),
            "comparison_class":"actual completed local draw journal; bind actual ReturnedCandidate",
        }),
        other => internal(other, aliases)?,
    })
}

fn serialize(case: &str, events: &[Observation], codec: &str) -> Result<Value, String> {
    let (aliases, bindings) = aliases(case)?;
    let mut next_call = 0_u64;
    let mut active = None;
    let mut active_is_attempt = false;
    let mut participants = Vec::new();
    let mut records = Vec::new();
    for (sequence, event) in events.iter().enumerate() {
        let mut value = match event {
            Observation::Trajectory(event) => match event.as_ref() {
                TrajectoryRecord::InitializeInput {
                    game,
                    context,
                    seed,
                    start,
                } => {
                    if active.is_some() {
                        return Err("nested native owner call".into());
                    }
                    if codec == CONTROL_HISTORY_CODEC {
                        participants = seed.seats.clone();
                    }
                    let actual_cards = seed
                        .cards
                        .iter()
                        .map(|card| card.card)
                        .collect::<std::collections::BTreeSet<_>>();
                    if actual_cards.len() != seed.cards.len()
                        || seed
                            .cards
                            .iter()
                            .any(|card| aliases.alias(card.card).is_none())
                        || actual_cards.len() != bindings.as_array().unwrap().len()
                    {
                        return Err(
                            "actual supplied seed does not exactly join the selected alias map"
                                .into(),
                        );
                    }
                    next_call += 1;
                    active = Some(next_call);
                    json!({"tag":"InitializeInput","input":projection::qualification_call(&projection::QualificationCall::GameInitializeScheduled {game:*game,context,seed,start:*start},&aliases,None)?})
                }
                TrajectoryRecord::AdvanceInput { source, input } => {
                    if active.is_some() {
                        return Err("nested native owner call".into());
                    }
                    next_call += 1;
                    active = Some(next_call);
                    json!({"tag":"AdvanceInput","source":projection::state(source,&aliases)?,"input":projection::input(input)})
                }
                TrajectoryRecord::Returned { source, result } => {
                    if active.is_none() || active_is_attempt {
                        return Err("returned candidate without exact owner input".into());
                    }
                    json!({"tag":"ReturnedCandidate","source_after_call":source.as_ref().map(|v|projection::state(v,&aliases)).transpose()?,"candidate":projection::candidate(result,source.as_deref(),&aliases)?,"adoption":"actual successful return on the shared immutable route; no recorder adoption"})
                }
                TrajectoryRecord::AdvanceAttemptInput { source, input } => {
                    if codec != CONTROL_HISTORY_CODEC || active.is_some() {
                        return Err("nested or foreign-family original advance attempt".into());
                    }
                    next_call += 1;
                    active = Some(next_call);
                    active_is_attempt = true;
                    json!({"tag":"AdvanceAttemptInput","source":projection::state(source,&aliases)?,"input":projection::input(input)})
                }
                TrajectoryRecord::AdvanceAttemptResult { source, result } => {
                    if codec != CONTROL_HISTORY_CODEC || active.is_none() || !active_is_attempt {
                        return Err(
                            "advance attempt result without its exact original attempt input"
                                .into(),
                        );
                    }
                    let actual_result = match result.as_ref() {
                        Ok(candidate) => {
                            json!({"tag":"Ok","candidate":projection::candidate(candidate,Some(source.as_ref()),&aliases)?})
                        }
                        Err(error) => projection::game_error(error),
                    };
                    json!({"tag":"AdvanceAttemptResult","source_after_call":projection::state(source,&aliases)?,"result":actual_result,"adoption":"actual original Result only; Err creates no accepted candidate; no second call"})
                }
                TrajectoryRecord::Evaluated {
                    label,
                    state,
                    result,
                } => {
                    if active.is_some() {
                        return Err("query checkpoint during an unfinished owner call".into());
                    }
                    let projected = projection::state(state, &aliases)?;
                    json!({"tag":"Checkpoint","label":label,"state":projected,
                        "common_values":common_values(state.execution(),&projected),
                        "actual_evaluation":actual_evaluation(result),
                        "comparison_class":"candidate-common-fields-only; actual query result from exact unchanged returned owner"})
                }
                TrajectoryRecord::CurrentInput { source, object } => {
                    if active.is_some() {
                        return Err("current query input during an unfinished owner call".into());
                    }
                    json!({"tag":"CurrentCharacteristicsInput", "query_method":"EffectWindow::current_characteristics", "query_object":projection::opaque(object), "source":projection::state(source,&aliases)?})
                }
                TrajectoryRecord::Current { state, result } => {
                    if active.is_some() {
                        return Err("current query during an unfinished owner call".into());
                    }
                    json!({"tag":"CurrentCharacteristicsQuery", "query_method":"EffectWindow::current_characteristics", "query_object":projection::opaque(result.object), "state":projection::state(state,&aliases)?, "actual_result":projection::current_characteristics(result), "comparison_class":"actual existing query result; join exact labelled source checkpoint and physical object"})
                }
                TrajectoryRecord::Checkpoint { label, state } => {
                    if active.is_some() {
                        return Err("checkpoint during an unfinished owner call".into());
                    }
                    let collections = (codec == CONTROL_HISTORY_CODEC)
                        .then(|| control_history_collections(state, &aliases));
                    let execution = state.execution();
                    let state = projection::state(state, &aliases)?;
                    let mut value = json!({"tag":"Checkpoint","label":label,"state":state,"common_values":common_values(execution,&state),"comparison_class":"candidate-common-fields-only; exact field and cut join required"});
                    if let Some(collections) = collections {
                        value["control_history_collections"] = collections;
                    }
                    value
                }
            },
            other if codec == CONTROL_HISTORY_CODEC => {
                control_history_internal(other, &aliases, &participants)?
            }
            other => internal(other, &aliases)?,
        };
        value["sequence"] = json!(sequence);
        value["call_id"] = json!(active.or_else(|| (next_call > 0).then_some(next_call)));
        records.push(value);
        if matches!(event, Observation::Trajectory(value) if matches!(value.as_ref(), TrajectoryRecord::Returned { .. } | TrajectoryRecord::AdvanceAttemptResult { .. }))
        {
            active = None;
            active_is_attempt = false;
        }
    }
    if active.is_some()
        || (next_call == 0
            && !events
                .iter()
                .any(|event| matches!(event, Observation::PureEvaluation(_))))
    {
        return Err("unfinished or absent native owner calls".into());
    }
    Ok(json!({
        "schema_version":1,"record_kind":codec,"status":"unqualified-candidate",
        "case_id":case,"alias_bindings":bindings,"events":records,
        "proof_limits":[
            "No replay, expected-output substitution or independent rules implementation.",
            "Operational cuts and failed tentative prefixes do not assert canonical publication or root adoption.",
            "Post-decrement shield/multiplier cuts and exact cleanup/shared-check suffixes are native diagnostics where Forge has no exact matching observation.",
            "Common claims require separate frozen primary expectations, exact source/input/runtime binding and a reviewed reference/native field join.",
            "No full card, game, configured format, host, durable replay or capacity qualification."
        ]
    }))
}

#[test]
#[ignore = "source-reviewed opt-in seven-case numeric capture; explicit case, output directory, codec and vocabulary input required"]
fn capture_selected_native_common_values() {
    capture_case(CaptureFamily::Common);
}

#[test]
#[ignore = "source-reviewed opt-in static17 typed capture; explicit case/output/codec/vocabulary and future V8 binding required"]
fn capture_selected_native_static_values() {
    capture_case(CaptureFamily::Static);
}

#[test]
#[ignore = "source-only resolved6 typed capture; explicit case/output/codec/vocabulary and future V9 binding required"]
fn capture_selected_native_resolved_values() {
    capture_case(CaptureFamily::Resolved);
}

#[test]
#[ignore = "source-only control-history2 capture; explicit case/output/codec/vocabulary and future V10 binding required"]
fn capture_selected_native_control_history_values() {
    capture_case(CaptureFamily::ControlHistory);
}

fn capture_case(family: CaptureFamily) {
    let codec = family.codec();
    let case = env::var("SPELLWAKE_CHARACTERISTICS_CAPTURE_CASE")
        .expect("explicit selected case required");
    let output = env::var_os("SPELLWAKE_CHARACTERISTICS_CAPTURE_OUTPUT")
        .expect("explicit fresh output directory required");
    assert_eq!(
        env::var("SPELLWAKE_CHARACTERISTICS_CAPTURE_CODEC").as_deref(),
        Ok(codec)
    );
    let vocabulary = env::var_os("SPELLWAKE_CREATURE_VOCABULARY_INPUT")
        .expect("explicit source-bound complete vocabulary required");
    let output = Path::new(&output);
    assert!(
        output.is_absolute() && output.is_dir(),
        "output must be an existing absolute directory"
    );
    assert!(
        fs::read_dir(output).unwrap().next().is_none(),
        "capture output directory must be empty"
    );
    // Validate case and bind all actual source bytes before executing the selected route.
    let _ = aliases(&case).expect("one of the finite source-reviewed comparison cases required");
    assert!(
        family.cases().contains(&case.as_str()),
        "case belongs to a different finite capture family"
    );
    let repository = current_capture::repository();
    let mut source_paths = vec![
        "crates/engine/src/characteristics.rs",
        "crates/engine/src/characteristics/tests.rs",
        "crates/engine/src/characteristics/evaluation.rs",
        "crates/engine/src/characteristics/ordering.rs",
        "crates/engine/src/characteristics/types.rs",
        "crates/engine/src/effects/characteristic_reference_capture.rs",
        "crates/engine/src/effects/characteristic_observations.rs",
        "crates/engine/src/effects/occurrence_flow.rs",
        "crates/engine/src/effects.rs",
        "crates/engine/test_support/characteristic_trajectories.rs",
        "crates/engine/test_support/creature_vocabulary.rs",
        "crates/engine/test_support/projection.rs",
        "crates/engine/test_support/program.rs",
        "crates/engine/test_support/damage_trace.rs",
        "crates/engine/test_support/check_trace.rs",
        "crates/engine/test_support/packet.rs",
    ];
    if matches!(family, CaptureFamily::ControlHistory) {
        source_paths.extend([
            "crates/engine/test_support/error.rs",
            "crates/engine/src/object_identity.rs",
            "crates/engine/src/draw.rs",
            "crates/engine/src/game.rs",
        ]);
    }
    let sources = source_paths
        .iter()
        .map(|path| current_capture::source(&repository.join(path)).unwrap())
        .collect::<Vec<_>>();
    let vocabulary_source = current_capture::source(Path::new(&vocabulary)).unwrap();
    let executable = current_capture::source(&env::current_exe().unwrap()).unwrap();
    let events = characteristic_observations::capture(|| {
        let observe: for<'a> fn(TrajectoryObservation<'a>) = match family {
            CaptureFamily::ControlHistory => control_history_trajectory,
            CaptureFamily::Resolved => resolved_trajectory,
            CaptureFamily::Common | CaptureFamily::Static => trajectory,
        };
        routes::with_trajectory_observer(observe, || route(&case));
    });
    let mut record =
        serialize(&case, &events, codec).expect("serialize exact typed native capture");
    let sources_after = source_paths
        .iter()
        .map(|path| current_capture::source(&repository.join(path)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        sources, sources_after,
        "capture source bytes changed during the selected route"
    );
    assert_eq!(
        vocabulary_source,
        current_capture::source(Path::new(&vocabulary)).unwrap(),
        "vocabulary input changed during the selected route"
    );
    assert_eq!(
        executable,
        current_capture::source(&env::current_exe().unwrap()).unwrap(),
        "executing binary changed during the selected route"
    );
    record["source_bindings"] = json!(sources);
    record["source_bindings_after"] = json!(sources_after);
    record["vocabulary_input"] = json!(vocabulary_source);
    record["executable"] = json!(executable);
    record["source_binding_scope"] = json!(
        "recorder and direct serialization dependencies only; separate whole native epoch binding required"
    );
    let bytes = serde_json::to_vec_pretty(&record).unwrap();
    // No output exists before all route assertions and serialization succeed.
    let pending = output.join("native-values.unaccepted.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .unwrap();
    file.write_all(&bytes).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
    println!("unqualified native value record: {}", pending.display());
}
