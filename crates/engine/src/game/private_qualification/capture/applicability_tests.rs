//! Real private adapter's pre-native `?` must not skip a denied native call.
use super::*;
use crate::test_support::{
    current_capture::applicability_tests::{Reset, denied, start},
    current_profile::applicability::tests as fixture,
};
use std::cell::Cell;

#[test]
fn applicability_private_question_mark_adapter_executes_original_and_replay_after_first_denial() {
    let _reset = Reset;
    let state = fixture::pending();
    let source = state.execution();
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    let frame = crate::effects::EffectDecision::ResolutionSelect(frame.clone());
    let reply = DecisionReply::SelectResolution(vec![]);
    let expected = source.resume_rule(source.revision(), &frame, fixture::A, reply.clone(), None);
    assert!(expected.is_err());
    let invoked = Cell::new(0);
    start();
    let actual = effect(
        source,
        &packet::AliasContext::default(),
        projection::QualificationCall::EffectCall(projection::EffectCall::ResumeRule {
            expected: source.revision(),
            frame: &frame,
            actor: fixture::A,
            reply: &reply,
            turn: None,
        }),
        |source| {
            invoked.set(invoked.get() + 1);
            source.resume_rule(source.revision(), &frame, fixture::A, reply.clone(), None)
        },
        |result| match result {
            Ok(value) => projection::rule_progress(value, &packet::AliasContext::default()),
            Err(error) => Ok(projection::effect_error(error)),
        },
        |value| Some(value.state()),
        |value| Some(value.append()),
        false,
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(invoked.get(), 2);
    denied();
}
