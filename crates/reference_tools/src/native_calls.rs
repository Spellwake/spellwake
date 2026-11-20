//! Typed caller adapters for actual qualification owners outside the root API.
use super::{
    current_capture,
    current_profile::{Append, CallPoint, Phase, ReplayEvidence, SnapshotOwner, SourceCapture},
    packet::AliasContext,
    projection,
};
use serde_json::json;
use spellwake_engine::{
    effects::{EffectError, EffectWindow},
    setup::GameSeed,
};
pub fn effect_initialize(seed: GameSeed) -> Result<EffectWindow, EffectError> {
    effect_initialize_with_adoption(seed, true)
}
pub fn effect_initialize_with_adoption(
    seed: GameSeed,
    adopt: bool,
) -> Result<EffectWindow, EffectError> {
    effect_initialize_recorded(seed, adopt).0
}
pub fn effect_initialize_recorded(
    seed: GameSeed,
    adopt: bool,
) -> (Result<EffectWindow, EffectError>, Option<String>) {
    if !current_capture::is_active() {
        return (EffectWindow::new(seed), None);
    }
    let saved = seed.clone();
    let aliases = AliasContext::default();
    let call_id =
        current_capture::native_call_id().expect("allocate native qualification call label");
    let before = current_capture::native_snapshot(
        Phase::InitializeInput,
        "EffectWindow::new",
        SnapshotOwner::InitializeInput {
            context: None,
            seed: &saved,
            source_ref: "actual supplied GameSeed",
        },
        &aliases,
    )
    .expect("observe actual supplied EffectWindow seed");
    let result = EffectWindow::new(seed);
    let replayed = EffectWindow::new(saved.clone());
    let after = result.as_ref().ok().map(|value| {
        current_capture::native_snapshot(
            Phase::AfterCall,
            "EffectWindow::new",
            SnapshotOwner::EffectWindow(value),
            &aliases,
        )
        .expect("observe actual initialized EffectWindow")
    });
    current_capture::record_native(
        CallPoint {
            call_id: &call_id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: adopt && result.is_ok(),
        },
        &projection::QualificationCall::EffectInitialize(&saved),
        &result,
        &replayed,
        |result| match result {
            Ok(value) => Ok(json!({"tag":"Ok","state":projection::effect(value,&aliases)?})),
            Err(error) => Ok(projection::effect_error(error)),
        },
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: None,
            source_capture: SourceCapture::InputOnlyForInitializer,
        },
        &aliases,
        None,
    )
    .expect("record actual typed EffectWindow initializer and full replay Eq");
    (result, Some(before))
}

use spellwake_engine::{
    object_identity::{GameObjects, ObjectChange, ObjectId, ObjectIdentityError},
    priority::SeatId,
};
pub fn card_turn_start(
    source: &GameObjects,
    expected: u64,
    player: SeatId,
    seats: &[SeatId],
) -> Result<GameObjects, ObjectIdentityError> {
    if !current_capture::is_active() {
        return source.record_turn_start(expected, player);
    }
    let saved = source.clone();
    let aliases = AliasContext::default();
    let call_id =
        current_capture::native_call_id().expect("allocate native qualification call label");
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        "GameObjects::record_turn_start",
        SnapshotOwner::GameObjects {
            cards: source,
            participants: seats,
        },
        &aliases,
    )
    .expect("observe actual physical source before call");
    let result = source.record_turn_start(expected, player);
    let replayed = source.record_turn_start(expected, player);
    let after = result.as_ref().ok().map(|cards| {
        current_capture::native_snapshot(
            Phase::AfterCall,
            "GameObjects::record_turn_start",
            SnapshotOwner::GameObjects {
                cards,
                participants: seats,
            },
            &aliases,
        )
        .expect("observe actual physical successor")
    });
    current_capture::record_native(
        CallPoint {
            call_id: &call_id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: result.is_ok(),
        },
        &projection::QualificationCall::GameObjectsCall(
            projection::GameObjectsCall::RecordTurnStart { expected, player },
        ),
        &result,
        &replayed,
        |result| {
            Ok(match result {
                Ok(cards) => json!({"tag":"Ok","state":projection::card_objects(cards,seats)}),
                Err(error) => projection::object_identity_error(error),
            })
        },
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(*source == saved),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        None,
    )
    .expect("record actual typed control-start primitive and full replay Eq");
    result
}
pub fn card_controller(
    source: &GameObjects,
    expected: u64,
    object: ObjectId,
    controller: SeatId,
    seats: &[SeatId],
) -> Result<ObjectChange, ObjectIdentityError> {
    if !current_capture::is_active() {
        return source.apply_controller(expected, object, controller);
    }
    let saved = source.clone();
    let aliases = AliasContext::default();
    let call_id =
        current_capture::native_call_id().expect("allocate native qualification call label");
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        "GameObjects::apply_controller",
        SnapshotOwner::GameObjects {
            cards: source,
            participants: seats,
        },
        &aliases,
    )
    .expect("observe actual physical source before call");
    let result = source.apply_controller(expected, object, controller);
    let replayed = source.apply_controller(expected, object, controller);
    let after = result.as_ref().ok().map(|change| {
        current_capture::native_snapshot(
            Phase::AfterCall,
            "GameObjects::apply_controller",
            SnapshotOwner::GameObjects {
                cards: &change.next,
                participants: seats,
            },
            &aliases,
        )
        .expect("observe actual physical successor")
    });
    current_capture::record_native(CallPoint{call_id:&call_id,before_point:Some(&before),after_point:after.as_deref(),adopted:result.is_ok()},&projection::QualificationCall::GameObjectsCall(projection::GameObjectsCall::ApplyController{expected,object,controller}),&result,&replayed,|result|Ok(match result{Ok(change)=>json!({"tag":"Ok","state":projection::card_objects(&change.next,seats),"before":change.before.as_ref().map(projection::object),"after":projection::object(&change.after)}),Err(error)=>projection::object_identity_error(error)}),Append::NotApplicable,ReplayEvidence{source_eq:Some(*source==saved),source_capture:SourceCapture::BeforeInvocation},&aliases,None).expect("record actual typed controller primitive and full replay Eq");
    result
}
