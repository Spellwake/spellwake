//! Actual owning-descendant controls over issued private creature type frames.
use super::*;
use crate::test_support::{
    current_capture,
    current_profile::{Append, CallPoint, Phase, ReplayEvidence, SnapshotOwner, SourceCapture},
    packet, projection, resolution_contract_cases as shared,
    resolution_contract_fixture as fixture,
};
use serde_json::{Value, json};
use std::cell::Cell;
const OWNED: &[&str] = &[
    "S2C-VM09-013",
    "S2C-VM09-014",
    "S2C-VM09-015",
    "S2C-VM09-016",
    "S2C-VM09-017",
    "S2C-VM09-018",
    "S2C-VM09-019",
];
#[derive(Debug, PartialEq, Eq)]
struct Native {
    public: fixture::PublicNative,
    original: CreatureTypeChoice,
    submitted: CreatureTypeChoice,
    result: Result<RuleProgress, EffectError>,
}
fn projected(value: &Result<RuleProgress, EffectError>) -> Value {
    match value {
        Ok(value) => {
            json!({"tag":"Ok","progress":projection::rule_progress(value,&packet::AliasContext::default()).expect("actual progress projection")})
        }
        Err(error) => projection::effect_error(error),
    }
}
fn invoke(
    source: &EffectWindow,
    expected: u64,
    frame: &EffectDecision,
    actor: SeatId,
    reply: &DecisionReply,
) -> Result<Result<RuleProgress, EffectError>, String> {
    if !current_capture::is_active() {
        return Ok(source.resume_rule(expected, frame, actor, reply.clone(), None));
    }
    let aliases = packet::AliasContext::default();
    let retained = source.clone();
    let id = current_capture::native_call_id()?;
    let before = current_capture::native_snapshot(
        Phase::BeforeCall,
        "EffectWindow::resume_rule",
        SnapshotOwner::EffectWindow(source),
        &aliases,
    )?;
    let result = source.resume_rule(expected, frame, actor, reply.clone(), None);
    let repeated = source.resume_rule(expected, frame, actor, reply.clone(), None);
    let after = result
        .as_ref()
        .ok()
        .map(|value| {
            current_capture::native_snapshot(
                Phase::AfterCall,
                "EffectWindow::resume_rule",
                SnapshotOwner::EffectWindow(value.state()),
                &aliases,
            )
        })
        .transpose()?;
    current_capture::record_native(
        CallPoint {
            call_id: &id,
            before_point: Some(&before),
            after_point: after.as_deref(),
            adopted: false,
        },
        &projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
            expected,
            frame,
            actor,
            reply,
            turn: None,
        }),
        &result,
        &repeated,
        |value| Ok(projected(value)),
        result
            .as_ref()
            .map(|value| Append::RulesJournal(value.append()))
            .unwrap_or(Append::NotApplicable),
        ReplayEvidence {
            source_eq: Some(*source == retained),
            source_capture: SourceCapture::BeforeInvocation,
        },
        &aliases,
        Some(source),
    )?;
    Ok(result)
}
fn execute(selection: &shared::Selection) -> Result<(Vec<shared::CallRecord>, Native), String> {
    let recipe = &selection.case.recipe;
    if recipe["op"] != "EffectWindow::resume_rule"
        || recipe["preserve_original_frame_separately"] != true
        || recipe["actor"] != "A"
    {
        return Err("unlisted private issued-frame control".into());
    }
    let (mut calls, public) =
        fixture::public_pending(&recipe["prepare"], selection.vocabulary.clone())?;
    let Some(crate::game::RuleFrame::CreatureType(issued)) =
        public.state.pending_decision().map(|value| value.frame())
    else {
        return Err("actual public preparation has no issued creature type frame".into());
    };
    let original = issued.clone();
    let mut submitted = original.clone();
    match selection.case.case_id.as_str() {
        "S2C-VM09-013" => {
            if recipe["replacement"] != "checked_issued_revision+1" {
                return Err("unlisted private frame revision delta".into());
            }
            submitted.revision = submitted
                .revision
                .checked_add(1)
                .ok_or("actual issued revision exhausted")?;
        }
        "S2C-VM09-014" => {
            if recipe["replacement"]
                != "actual lower-stack entry from separately accepted cast receipt"
            {
                return Err("unlisted private entry delta".into());
            }
            submitted.entry = public
                .lower_entry
                .ok_or("actual accepted lower cast entry missing")?;
        }
        "S2C-VM09-015" => {
            if recipe["replacement"] != "1 (the actual DrawCount instruction index)" {
                return Err("unlisted private instruction delta".into());
            }
            submitted.instruction = 1;
        }
        "S2C-VM09-016" => {
            if recipe["replacement"] != "CreatureTypeSlot::new(1)" {
                return Err("unlisted private slot delta".into());
            }
            submitted.slot = crate::program::CreatureTypeSlot::new(1);
        }
        "S2C-VM09-017" => {
            if recipe["replacement"] != "B" {
                return Err("unlisted private chooser delta".into());
            }
            submitted.chooser = SeatId(1);
        }
        "S2C-VM09-018" => {
            if recipe["replacement"] != "Vforeign.id" {
                return Err("unlisted private vocabulary delta".into());
            }
            submitted.vocabulary = fixture::foreign(&selection.vocabulary)?.id();
        }
        "S2C-VM09-019" => {
            if recipe["replacement"] != "{valid Wall ID}" {
                return Err("unlisted private exclusion delta".into());
            }
            submitted.excluding = std::collections::BTreeSet::from([selection
                .vocabulary
                .resolve_label("Wall")
                .ok_or("actual Wall member missing")?]);
        }
        _ => return Err("case not owned by frame descendant entry".into()),
    }
    let other = fixture::foreign(&selection.vocabulary)?;
    let source = public.state.execution();
    let frame = EffectDecision::CreatureType(submitted.clone());
    let reply = DecisionReply::CreatureType(fixture::lookup(
        &recipe["reply"],
        &selection.vocabulary,
        &other,
    )?);
    let args = (source.revision(), frame, SeatId(0), reply);
    let first = Cell::new(true);
    let (result, record) = shared::borrowed_call(
        "EffectWindow::resume_rule",
        source,
        &args,
        |source, (expected, frame, actor, reply)| {
            if first.replace(false) {
                invoke(source, *expected, frame, *actor, reply)
                    .expect("actual frame control capture")
            } else {
                current_capture::without_capture(|| {
                    source.resume_rule(*expected, frame, *actor, reply.clone(), None)
                })
            }
        },
        |source| {
            projection::effect(source, &packet::AliasContext::default())
                .expect("actual frame-control source")
        },
        |(expected, frame, actor, reply)| json!({"expected":expected.to_string(),"original_frame":projection::creature_type_frame(&original),"submitted_frame":projection::effect_frame(frame,source).expect("actual submitted frame"),"actor":actor.0,"reply":projection::reply(reply),"turn":null}),
        projected,
    );
    calls.push(record);
    Ok((
        calls,
        Native {
            public,
            original,
            submitted,
            result,
        },
    ))
}
#[test]
#[ignore = "requires exact frozen221 input/case/create-new output and actual D06 vocabulary/publication; private fields remain in owning cfg(test) descendant"]
fn selected_contract_case() {
    let selection = shared::Selection::from_env(OWNED).expect("exact private frame selection");
    let profile = "resolution-contract-cases";
    let root = current_capture::repository();
    current_capture::start(
        profile,
        &[
            std::path::PathBuf::from(&selection.input.path),
            root.join("crates/engine/src/effects/private_qualification/contract_cases.rs"),
            root.join("crates/engine/test_support/resolution_contract_fixture.rs"),
            root.join("tests/support/resolution_contract_cases.rs"),
            root.join(
                "work/resolution-value-choices/contract-case-preparation-v1/recipes.input.json",
            ),
        ],
        None,
    )
    .expect("actual source-bound private frame capture");
    let mut invocation = 0;
    let result = shared::run_with_finish(
        &selection,
        || {
            invocation += 1;
            if invocation == 1 {
                execute(&selection)
            } else {
                current_capture::without_capture(|| execute(&selection))
            }
        },
        |record| current_capture::finish_record(profile, record),
    );
    assert!(
        result.is_ok(),
        "actual private frame recipe failed: {result:?}"
    );
}
