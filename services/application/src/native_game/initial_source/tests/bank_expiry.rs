//! Real source/opening/checkpoint replay with a strict byte-read Storage mock.
//! This does not authenticate bank exhaustion, SQL acceptance or live Storage.
use super::*;

#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary; private bank checkpoint replay only"]
fn bank_expiry_checkpoint_survives_exact_cold_reconstruction() {
    let mut f = checkpoint_fixture(None);
    let viewer = seats()[0];
    let binding = f.tip.binding(viewer).unwrap();
    let source = f.tip.state();
    let holder = match source.execution().window().phase() {
        spellwake_engine::stack_window::WindowPhase::Priority(
            spellwake_engine::priority::PriorityPhase::Ready { holder },
        ) => holder,
        _ => panic!("actual fixture must have settled priority"),
    };
    let loser = binding
        .seats
        .iter()
        .find(|seat| seat.native_seat == holder)
        .unwrap()
        .seat_id;
    let prepared = NativeScope::new(source, binding)
        .unwrap()
        .prepare_bank_expiry(loser)
        .unwrap();
    let expected_candidate = prepared.candidate().clone();
    let bytes = prepared.record().to_vec();
    let input = NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::Input,
        &bytes,
    )
    .unwrap();
    let counts: Vec<_> = seats().iter().map(|seat| f.tip.own_turns(*seat)).collect();
    // A bank record cannot be reclassified as player input or entropy.
    for wrong in [
        NativeCheckpointInput::Player,
        NativeCheckpointInput::Entropy,
    ] {
        assert!(
            with_store(&bytes, &input, |store| f.tip.append(
                store,
                input.clone(),
                wrong,
                viewer
            ))
            .is_err()
        );
    }
    let next = with_store(&bytes, &input, |store| {
        f.tip.append(
            store,
            input.clone(),
            NativeCheckpointInput::BankExpiry,
            viewer,
        )
    })
    .unwrap();
    assert_eq!(next.candidate(), &expected_candidate);
    assert_eq!(
        serde_json::from_slice::<Value>(next.original_bytes()).unwrap()["version"],
        "native-bank-checkpoint-v1"
    );
    assert_eq!(
        counts,
        seats()
            .iter()
            .map(|seat| next.own_turns(*seat))
            .collect::<Vec<_>>()
    );
    f.inputs.push((input, bytes));
    f.states
        .push((next.reference().clone(), next.original_bytes().to_vec()));
    f.candidates.push(next.candidate().clone());
    f.tip = next;
    let cold = results::replay(&f);
    assert_eq!(cold.state(), f.tip.state());
    assert_eq!(cold.candidate(), f.tip.candidate());
    assert!(cold.reference() == f.tip.reference());
    assert_eq!(cold.original_bytes(), f.tip.original_bytes());
    assert_eq!(
        cold.prepare_results().unwrap().original_bytes(),
        f.tip.prepare_results().unwrap().original_bytes()
    );
    assert_eq!(
        counts,
        seats()
            .iter()
            .map(|seat| cold.own_turns(*seat))
            .collect::<Vec<_>>()
    );
    assert!(matches!(
        cold.retained_input().unwrap().0,
        NativeCheckpointInput::BankExpiry
    ));
}
