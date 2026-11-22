//! Genuine initialized ownership and original reconstruction, not clock wall time.
use super::*;
use crate::native_game::clocks::{Waiting, waiting};
#[test]
#[ignore = "requires genuine catalogue/vocabulary; initial ownership mechanism only"]
fn initial_clock_observation_requires_no_fake_input_and_preserves_original_reconstruction() {
    let programs = source(NativeProgramPool::ResolutionSelectionNaturalMana);
    let opening = prepare_initial(ids(), programs, spec()).unwrap();
    let bytes = opening.original_bytes().to_vec();
    let initial_ref = opening.reference().clone();
    let first = NativeCheckpoint::initial(opening).unwrap();
    let before = first.original_bytes().to_vec();
    let reference = first.reference().clone();
    let observed = serde_json::to_value(first.opening_clock_observation().unwrap()).unwrap();
    let native = first.state().pending_decision().unwrap().chooser();
    let active = first.state().execution().window().active_player();
    assert_eq!(
        observed["seatId"],
        json!(seats()[usize::try_from(native.0).unwrap()])
    );
    assert_eq!(observed["acceptedRevision"], "0");
    assert_eq!(observed["proofRef"], json!(reference));
    assert_eq!(first.basis().revision.get(), 0);
    assert!(first.retained_input().is_none());
    assert_eq!(first.original_bytes(), before);
    assert!(first.reference() == &reference);
    let restored = read_root(&bytes, &expected(initial_ref), programs).unwrap();
    let again = NativeCheckpoint::initial(restored).unwrap();
    assert!(again.reference() == first.reference());
    assert_eq!(
        serde_json::to_value(again.opening_clock_observation().unwrap()).unwrap(),
        observed
    );
    let viewer = seats()[usize::try_from(native.0).unwrap()];
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
    assert!(next.opening_clock_observation().is_err());
    let chooser = next.state().pending_decision().unwrap().chooser();
    assert_ne!(chooser, native);
    let expected = seats()[usize::try_from(chooser.0).unwrap()];
    assert!(
        matches!(waiting(next.state(),&next.binding(viewer).unwrap()).unwrap(),Waiting::Decision(seat) if seat==expected)
    );
    assert_eq!(next.state().execution().window().active_player(), active);
    assert!(native != active || chooser != active);
}
