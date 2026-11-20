//! Real native-packet adapter's pre-native unwrap remains observational only.
use super::*;
use crate::test_support::{
    current_capture::applicability_tests::{Reset, denied, start},
    current_profile::applicability::tests as fixture,
};

#[test]
fn applicability_packet_unwrap_adapter_keeps_successful_native_result_after_first_denial() {
    let _reset = Reset;
    let state = fixture::pending();
    let source = state.execution();
    let retained = source.clone();
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        unreachable!()
    };
    let reply = DecisionReply::SelectResolution(vec![frame.domain()[0]]);
    let decision = EffectDecision::ResolutionSelect(frame.clone());
    let expected = source.resume_rule(
        source.revision(),
        &decision,
        fixture::A,
        reply.clone(),
        None,
    );
    assert!(expected.is_ok());
    start();
    let result = super::native_packets::resume_rule(
        source,
        source.revision(),
        &decision,
        fixture::A,
        reply,
        None,
    );
    assert_eq!(result, expected);
    assert_eq!(source, &retained);
    denied();
}
