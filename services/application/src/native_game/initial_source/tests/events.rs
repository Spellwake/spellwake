//! Actual native candidate Event binding; Storage here is strict byte-read mock.
use super::*;

#[test]
#[ignore = "requires genuine catalogue/vocabulary; native Event mechanism only"]
fn actual_initial_and_successor_events_bind_exact_state_and_refuse_substitution_before_io() {
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let first =
        NativeCheckpoint::initial(prepare_initial(ids(), programs, spec()).unwrap()).unwrap();
    let original = first.original_bytes().to_vec();
    let event = first.prepare_event().unwrap();
    assert_eq!(event.reference().role().as_str(), "event");
    assert_eq!(event.reference().game_id(), first.basis().game_id);
    assert_eq!(
        serde_json::from_slice::<Value>(event.original_bytes()).unwrap(),
        json!({"version":"native-transition-v1","stateRef":first.reference()})
    );
    with_store(event.original_bytes(), event.reference(), |store| {
        first.verify_event(store, event.reference())
    })
    .unwrap();
    let chooser = first.state().pending_decision().unwrap().chooser();
    let viewer = seats()[usize::try_from(chooser.0).unwrap()];
    let mut scope = NativeScope::new(first.state(), first.binding(viewer).unwrap()).unwrap();
    let decision = scope
        .issue(NativeReference::Decision(
            first.state().pending_decision().unwrap().id(),
        ))
        .unwrap();
    let option = scope
        .issue(NativeReference::Mulligan(MulliganChoice::Keep))
        .unwrap();
    let proposal = scope
        .propose(&PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action: PlayerAction::Choose {
                decision,
                response: PlayerChoice::Option { option },
            },
        })
        .unwrap();
    let input_bytes = scope.encode_input(&proposal).unwrap();
    let input = NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::Input,
        &input_bytes,
    )
    .unwrap();
    let next = with_store(&input_bytes, &input, |store| {
        first.append(store, input.clone(), NativeCheckpointInput::Player, viewer)
    })
    .unwrap();
    let successor = next.prepare_event().unwrap();
    assert!(successor.reference() != event.reference());
    assert_eq!(first.original_bytes(), original);
    let mock = Mock::new(vec![]);
    let store = checkpoint_store(&mock);
    assert!(first.verify_event(&store, successor.reference()).is_err());
    assert!(next.verify_event(&store, event.reference()).is_err());
    assert!(next.verify_event(&store, next.reference()).is_err());
    assert!(mock.finish().is_empty());
    let mut altered = successor.original_bytes().to_vec();
    altered[0] ^= 1;
    assert!(
        with_store(&altered, successor.reference(), |store| next
            .verify_event(store, successor.reference()))
        .is_err()
    );
}
