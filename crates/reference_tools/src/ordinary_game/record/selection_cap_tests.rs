//! Existing Recorder limiter mechanism only. Selected Report5 execution remains refused.
use super::*;
use crate::ordinary_game::selection::{SelectionCase, policy_tests};
#[test]
#[ignore = "requires genuine source; real Recorder cumulative-cap mechanism, no Report5 run"]
fn selected_outer_cap_reaches_real_recorder_limit_before_any_projection_or_native_call() {
    let run = policy_tests::reached(SelectionCase::Sd04, "primary.select.P1");
    assert_eq!(run.policy.reached_probe_limit(), Some(64));
    let prepared = run.policy.prepare(&run.state).unwrap().unwrap();
    let probes = prepared.recipe_probes();
    assert!(!probes.is_empty());
    let mut recorder = Recorder::new(run.fixture.case);
    // Seed only this private mechanism's retained-count premise from64 actual
    // immutable native rejection calls. This is not an accepted Report5 history.
    for (index, probe) in probes.iter().cycle().take(64).enumerate() {
        let source = run.state.clone();
        let result = run.state.advance(probe.input.clone());
        assert_eq!(result.unwrap_err(), probe.expected);
        assert_eq!(run.state, source);
        recorder.saved_probes.push(SavedProbe {
            ordinal: usize::try_from(run.state.basis().revision).unwrap(),
            id: format!("{}.mechanism.{index}", probe.id),
            input: probe.input.clone(),
            expected: probe.expected.clone(),
        });
    }
    let before_state = run.state.clone();
    let before_policy = run.policy.selection_observation().unwrap();
    let before_saved = recorder.saved_probes.len();
    crate::current_capture::with_selection_observer_for_test(
        &run.fixture.setup.seed.creature_vocabulary,
        || {
            let before = crate::current_capture::caller_delivery_for_test();
            let result = recorder.probes(&run.fixture, &run.policy, &run.state, Some(&prepared));
            assert_eq!(
                result.unwrap_err(),
                "native fixture reached rejection probe budget exhausted"
            );
            assert_eq!(crate::current_capture::caller_delivery_for_test(), before);
        },
    );
    assert_eq!(recorder.saved_probes.len(), before_saved);
    assert_eq!(run.state, before_state);
    assert_eq!(run.policy.selection_observation().unwrap(), before_policy);
    assert!(recorder.report.accepted.is_empty());
    assert!(recorder.report.initialization.is_none());
}
