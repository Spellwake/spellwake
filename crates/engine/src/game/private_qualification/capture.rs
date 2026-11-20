//! Actual-owner observation at existing private caller seams.
use super::*;
use crate::test_support::{
    current_capture,
    current_profile::{Append, CallPoint, Phase, ReplayEvidence, SnapshotOwner, SourceCapture},
};

#[allow(clippy::too_many_arguments)]
pub(super) fn effect<
    R: Eq + crate::test_support::current_profile::applicability::NativeCaptureValue,
>(
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    input: projection::QualificationCall<'_>,
    call: impl Fn(&EffectWindow) -> Result<R, EffectError>,
    project: impl Fn(&Result<R, EffectError>) -> Result<Value, String>,
    successor: impl Fn(&R) -> Option<&EffectWindow>,
    append: impl Fn(&R) -> Option<&crate::occurrence::RulesJournal>,
    adopt: bool,
) -> Result<Result<R, EffectError>, String> {
    if !current_capture::is_active() {
        return Ok(call(source));
    }
    let retained = source.clone();
    let id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        input.owner_method(),
        SnapshotOwner::EffectWindow(source),
        aliases,
    )?;
    let result = call(source);
    let repeated = call(source);
    let after = match result.as_ref().ok().and_then(&successor) {
        Some(state) => Some(current_capture::native_snapshot(
            Phase::AfterCall,
            input.owner_method(),
            SnapshotOwner::EffectWindow(state),
            aliases,
        )?),
        None => None,
    };
    current_capture::record_native(
        CallPoint {
            call_id: &id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: adopt && result.is_ok(),
        },
        &input,
        &result,
        &repeated,
        project,
        result
            .as_ref()
            .ok()
            .and_then(append)
            .map(Append::RulesJournal)
            .unwrap_or(Append::NotApplicable),
        ReplayEvidence {
            source_eq: Some(*source == retained),
            source_capture: SourceCapture::BeforeInvocation,
        },
        aliases,
        Some(source),
    )?;
    Ok(result)
}
pub(super) fn progress(
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    input: projection::EffectCall<'_>,
    call: impl Fn(&EffectWindow) -> Result<RuleProgress, EffectError>,
    adopt: bool,
) -> Result<Result<RuleProgress, EffectError>, String> {
    effect(
        source,
        aliases,
        projection::QualificationCall::EffectCall(input),
        call,
        |result| match result {
            Ok(value) => {
                Ok(json!({"tag":"Ok","progress":projection::rule_progress(value,aliases)?}))
            }
            Err(error) => Ok(projection::effect_error(error)),
        },
        |value| Some(value.state()),
        |value| Some(value.append()),
        adopt,
    )
}
pub(super) fn state(
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    input: projection::EffectCall<'_>,
    call: impl Fn(&EffectWindow) -> Result<EffectWindow, EffectError>,
    adopt: bool,
) -> Result<Result<EffectWindow, EffectError>, String> {
    effect(
        source,
        aliases,
        projection::QualificationCall::EffectCall(input),
        call,
        |result| match result {
            Ok(value) => Ok(json!({"tag":"Ok","state":projection::effect(value,aliases)?})),
            Err(error) => Ok(projection::effect_error(error)),
        },
        |value| Some(value),
        |_| None,
        adopt,
    )
}
pub(super) fn transition<
    T: Eq + crate::test_support::current_profile::applicability::NativeCaptureValue,
>(
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    input: projection::EffectCall<'_>,
    call: impl Fn(
        &EffectWindow,
    ) -> Result<crate::occurrence::RulesTransition<EffectWindow, T>, EffectError>,
    result_value: impl Fn(&T) -> Value,
    adopt: bool,
) -> Result<Result<crate::occurrence::RulesTransition<EffectWindow, T>, EffectError>, String> {
    effect(
        source,
        aliases,
        projection::QualificationCall::EffectCall(input),
        call,
        |result| match result {
            Ok(value) => Ok(
                json!({"tag":"Ok","state":projection::effect(value.state(),aliases)?,"result":result_value(value.result()),"append":projection::journal(value.append())}),
            ),
            Err(error) => Ok(projection::effect_error(error)),
        },
        |value| Some(value.state()),
        |value| Some(value.append()),
        adopt,
    )
}
pub(super) fn effect_point(
    mut view: Value,
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    label: &str,
) -> Result<Value, String> {
    if current_capture::is_active() {
        let point = current_capture::native_snapshot(
            Phase::Observation,
            label,
            SnapshotOwner::EffectWindow(source),
            aliases,
        )?;
        view["__current_capture_point"] = json!(point);
    }
    Ok(view)
}
pub(super) fn effect_record_point(
    mut view: Value,
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    label: &str,
    storage: &str,
) -> Result<Value, String> {
    if let Some(point) = crate::test_support::capture_points::token(
        label,
        SnapshotOwner::EffectWindow(source),
        aliases,
    )? {
        current_capture::reference_record_point(&point, storage)?;
        view["__current_capture_point"] = json!(point);
    }
    Ok(view)
}

pub(super) fn begin(path: &Path, profile: &str, matrix: Option<&Path>) -> Result<(), String> {
    let root = current_capture::repository();
    let mut sources = vec![path.to_path_buf()];
    if let Some(path) = matrix {
        sources.push(path.to_path_buf());
    }
    for source in [
        "crates/engine/src/game/private_qualification.rs",
        "crates/engine/src/game/private_qualification/capture.rs",
        "crates/engine/src/game/private_qualification/input.rs",
        "crates/engine/src/game/private_qualification/root.rs",
        "crates/engine/src/game/private_qualification/trusted.rs",
        "crates/engine/src/game/private_qualification/v2.rs",
        "crates/engine/src/game/private_qualification/v2/definitions.rs",
        "crates/engine/src/game/private_qualification/v2/matcher.rs",
        "crates/engine/src/game/private_qualification/v2/executor.rs",
        "crates/engine/src/game/private_qualification/v2/registry.rs",
        "crates/engine/src/game/private_qualification/v2/raw_window.rs",
        "crates/engine/src/game/private_qualification/v2/body.rs",
        "crates/engine/src/game/private_qualification/root/extended.rs",
        "crates/engine/src/game/private_qualification/v2/admission.rs",
        "crates/engine/src/game/private_qualification/v2/construction.rs",
        "crates/engine/src/game/private_qualification/v2/raw_window/resolution.rs",
        "crates/engine/src/game/private_qualification/v2/body/actions.rs",
        "crates/engine/src/game/private_qualification/v2/body/operations.rs",
        "crates/engine/src/game/private_qualification/v2/body/resolution_values.rs",
        "crates/engine/src/game/private_qualification/v2/body/resolution_values/root_faults.rs",
        "crates/engine/src/game/private_qualification/v2/body/resolution_values/guards.rs",
    ] {
        sources.push(root.join(source));
    }
    current_capture::start(profile, &sources, None)
}
pub(super) fn bind(view: &mut Value) -> Result<(), String> {
    crate::test_support::capture_points::bind_value_points(view, "/observation")
}
pub(super) fn finish(profile: &str, report: &mut Value) {
    if !current_capture::is_active() {
        return;
    }
    match current_capture::finish_record(profile, report.clone()) {
        Ok(mut value) => {
            if value["type_context_witness"]["calls"]
                .as_array()
                .is_none_or(Vec::is_empty)
            {
                value["status"] = json!("failed");
                value["producer_integrity"] = json!(false);
                value["witness_failure"] =
                    json!("no actual native calls were recorded for this selected private scope");
            }
            *report = value;
        }
        Err(error) => {
            report["status"] = json!("failed");
            report["producer_integrity"] = json!(false);
            report["witness_failure"] = json!(error);
        }
    }
}

pub(super) fn validate_program(
    program: &crate::program::Program,
    vocabulary: &spellwake_contracts::creature_types::CreatureTypeVocabulary,
) -> Result<Result<(), crate::program::DefinitionError>, String> {
    if !current_capture::is_active() {
        return Ok(program.validate(vocabulary));
    }
    let retained = program.clone();
    let retained_vocabulary = vocabulary.clone();
    let aliases = packet::AliasContext::default();
    let call_id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::PureInput,
        "Program::validate",
        SnapshotOwner::ProgramInput {
            program,
            source_ref: "actual selected/mutated native Program input",
        },
        &aliases,
    )?;
    let result = program.validate(vocabulary);
    let repeated = program.validate(vocabulary);
    let project = |result: &Result<(), crate::program::DefinitionError>| {
        Ok::<Value, String>(match result {
            Ok(()) => json!({"tag":"Ok"}),
            Err(error) => projection::definition_error(error),
        })
    };
    let after = if result.is_ok() {
        let value = project(&result)?;
        Some(current_capture::native_snapshot(
            Phase::PureResult,
            "Program::validate",
            SnapshotOwner::PureValue(&value),
            &aliases,
        )?)
    } else {
        None
    };
    current_capture::record_native(
        CallPoint {
            call_id: &call_id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: false,
        },
        &projection::QualificationCall::ProgramValidate {
            program,
            vocabulary,
        },
        &result,
        &repeated,
        project,
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(*program == retained && *vocabulary == retained_vocabulary),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        None,
    )?;
    Ok(result)
}

pub(super) fn pure<
    R: Eq + crate::test_support::current_profile::applicability::NativeCaptureValue,
    E: Eq,
>(
    method: &str,
    input: &Value,
    call: impl Fn() -> Result<R, E>,
    project: impl Fn(&Result<R, E>) -> Result<Value, String>,
    input_eq: impl Fn() -> bool,
) -> Result<Result<R, E>, String> {
    if !current_capture::is_active() {
        return Ok(call());
    }
    let aliases = packet::AliasContext::default();
    let call_id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::PureInput,
        method,
        SnapshotOwner::PureValue(input),
        &aliases,
    )?;
    let result = call();
    let repeated = call();
    let after = if result.is_ok() {
        let value = project(&result)?;
        Some(current_capture::native_snapshot(
            Phase::PureResult,
            method,
            SnapshotOwner::PureValue(&value),
            &aliases,
        )?)
    } else {
        None
    };
    current_capture::record_native(
        CallPoint {
            call_id: &call_id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: false,
        },
        &projection::QualificationCall::PureCall { method, input },
        &result,
        &repeated,
        project,
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(input_eq()),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        None,
    )?;
    Ok(result)
}

pub(super) fn root_point(
    mut view: Value,
    source: &GameState,
    aliases: &packet::AliasContext,
    label: &str,
    storage: &str,
) -> Result<Value, String> {
    if let Some(point) = crate::test_support::capture_points::token(
        label,
        SnapshotOwner::GameState(source),
        aliases,
    )? {
        current_capture::reference_record_point(&point, storage)?;
        view["__current_capture_point"] = json!(point);
    }
    Ok(view)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn card<
    R: Eq + crate::test_support::current_profile::applicability::NativeCaptureValue,
>(
    source: &crate::object_identity::GameObjects,
    seats: &[SeatId],
    input: projection::GameObjectsCall<'_>,
    call: impl Fn(
        &crate::object_identity::GameObjects,
    ) -> Result<R, crate::object_identity::ObjectIdentityError>,
    project: impl Fn(&Result<R, crate::object_identity::ObjectIdentityError>) -> Result<Value, String>,
    successor: impl Fn(&R) -> Option<&crate::object_identity::GameObjects>,
    adopt: bool,
) -> Result<Result<R, crate::object_identity::ObjectIdentityError>, String> {
    if !current_capture::is_active() {
        return Ok(call(source));
    }
    let aliases = packet::AliasContext::default();
    let retained = source.clone();
    let input = projection::QualificationCall::GameObjectsCall(input);
    let id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        input.owner_method(),
        SnapshotOwner::GameObjects {
            cards: source,
            participants: seats,
        },
        &aliases,
    )?;
    let result = call(source);
    let repeated = call(source);
    let after = match result.as_ref().ok().and_then(successor) {
        Some(cards) => Some(current_capture::native_snapshot(
            Phase::AfterCall,
            input.owner_method(),
            SnapshotOwner::GameObjects {
                cards,
                participants: seats,
            },
            &aliases,
        )?),
        None => None,
    };
    current_capture::record_native(
        CallPoint {
            call_id: &id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: adopt && result.is_ok(),
        },
        &input,
        &result,
        &repeated,
        project,
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(*source == retained),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        None,
    )?;
    Ok(result)
}

pub(super) fn add_effect_record_point(
    mut view: Value,
    source: &EffectWindow,
    aliases: &packet::AliasContext,
    label: &str,
    storage: &str,
) -> Result<Value, String> {
    if let Some(point) = crate::test_support::capture_points::token(
        label,
        SnapshotOwner::EffectWindow(source),
        aliases,
    )? {
        current_capture::reference_record_point(&point, storage)?;
        let previous = view["__current_capture_point"].take();
        view["__current_capture_point"] = match previous {
            Value::String(previous) => json!([previous, point]),
            Value::Array(mut points) => {
                points.push(json!(point));
                json!(points)
            }
            Value::Null => json!(point),
            _ => return Err("invalid existing private capture label".into()),
        };
    }
    Ok(view)
}

pub(super) fn window<
    R: Eq + crate::test_support::current_profile::applicability::NativeCaptureValue,
>(
    source: &crate::stack_window::StackWindow,
    input: projection::StackWindowCall<'_>,
    call: impl Fn(&crate::stack_window::StackWindow) -> Result<R, crate::stack_window::StackWindowError>,
    project: impl Fn(&Result<R, crate::stack_window::StackWindowError>) -> Result<Value, String>,
    successor: impl Fn(&R) -> Option<crate::stack_window::StackWindow>,
    adopt: bool,
) -> Result<Result<R, crate::stack_window::StackWindowError>, String> {
    if !current_capture::is_active() {
        return Ok(call(source));
    }
    let aliases = packet::AliasContext::default();
    let retained = source.clone();
    let input = projection::QualificationCall::StackWindowCall(input);
    let id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        input.owner_method(),
        SnapshotOwner::StackWindow(source),
        &aliases,
    )?;
    let result = call(source);
    let repeated = call(source);
    let returned = result.as_ref().ok().and_then(successor);
    let after = match returned.as_ref() {
        Some(window) => Some(current_capture::native_snapshot(
            Phase::AfterCall,
            input.owner_method(),
            SnapshotOwner::StackWindow(window),
            &aliases,
        )?),
        None => None,
    };
    current_capture::record_native(
        CallPoint {
            call_id: &id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: adopt && result.is_ok(),
        },
        &input,
        &result,
        &repeated,
        project,
        Append::NotApplicable,
        ReplayEvidence {
            source_eq: Some(*source == retained),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        None,
    )?;
    Ok(result)
}
pub(super) fn window_point(
    mut view: Value,
    source: &crate::stack_window::StackWindow,
    label: &str,
    storage: Option<&str>,
) -> Result<Value, String> {
    if current_capture::is_active() {
        let aliases = packet::AliasContext::default();
        let point = if let Some(storage) = storage {
            let point =
                current_capture::observation(label, SnapshotOwner::StackWindow(source), &aliases)?;
            current_capture::reference_record_point(&point, storage)?;
            point
        } else {
            current_capture::native_snapshot(
                Phase::Observation,
                label,
                SnapshotOwner::StackWindow(source),
                &aliases,
            )?
        };
        view["__current_capture_point"] = json!(point);
    }
    Ok(view)
}

pub(super) fn card_point(
    mut view: Value,
    source: &crate::object_identity::GameObjects,
    seats: &[SeatId],
    label: &str,
    storage: Option<&str>,
) -> Result<Value, String> {
    if current_capture::is_active() {
        let aliases = packet::AliasContext::default();
        let owner = SnapshotOwner::GameObjects {
            cards: source,
            participants: seats,
        };
        let point = if let Some(storage) = storage {
            let point = current_capture::observation(label, owner, &aliases)?;
            current_capture::reference_record_point(&point, storage)?;
            point
        } else {
            current_capture::native_snapshot(Phase::Observation, label, owner, &aliases)?
        };
        view["__current_capture_point"] = json!(point);
    }
    Ok(view)
}

pub(super) fn attach_writer_point(mut view: Value, point: Option<String>) -> Value {
    if let Some(point) = point {
        view["__current_capture_point"] = json!(point);
    }
    view
}

#[cfg(test)]
mod applicability_tests;
