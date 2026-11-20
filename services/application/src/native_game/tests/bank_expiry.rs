//! Finite private mechanics/reconstruction, not SQL clock authorization.
use super::*;
use serde_json::{Value, json};

#[test]
#[ignore = "requires explicit genuine vocabulary; private bank replay mechanics only"]
fn bank_expiry_replays_exactly_and_refuses_wrong_source_kind_version_and_seat() {
    let source = state(vec![]);
    let before = source.clone();
    let view = scope(&source, viewer_b());
    // A trusted host binding's viewer does not grant the nonwaiting viewer expiry.
    assert!(view.prepare_bank_expiry(viewer_b()).is_err());
    assert!(view.prepare_bank_expiry(Uuid::from_u128(9999)).is_err());
    let prepared = view.prepare_bank_expiry(viewer_a()).unwrap();
    let direct = source
        .expire_bank(spellwake_engine::game::BankExpiryInput {
            basis: source.basis().clone(),
            player: A,
        })
        .unwrap();
    assert_eq!(prepared.candidate(), &direct);
    let restored =
        restore_bank_expiry(&source, binding(&source, viewer_b()), prepared.record()).unwrap();
    assert_eq!(restored, direct);
    assert_eq!(source, before);
    let terminal = NativeTerminal::derive(restored.state(), &binding(restored.state(), viewer_b()))
        .unwrap()
        .unwrap();
    assert_eq!(terminal.reason(), "native_bank_expired");
    assert_eq!(terminal.winner_seat_ids(), &[viewer_b()]);
    assert_eq!(terminal.losing_seat_ids(), &[viewer_a()]);
    let original: Value = serde_json::from_slice(prepared.record()).unwrap();
    assert_eq!(original["version"], "native-bank-input-v1");
    assert_eq!(
        original["transition"],
        json!({"kind":"bank-expiry","seatId":viewer_a()})
    );
    for (pointer, replacement) in [
        ("/version", json!("native-input-v1")),
        ("/transition/kind", json!("player")),
        ("/transition/seatId", json!(viewer_b())),
        ("/transition/seatId", json!(Uuid::nil())),
        ("/source/basis/revision", json!("999")),
    ] {
        let mut corrupt = original.clone();
        *corrupt.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            restore_bank_expiry(
                &source,
                binding(&source, viewer_b()),
                &serde_json::to_vec(&corrupt).unwrap()
            )
            .is_err()
        );
        assert_eq!(source, before);
    }
    let mut extra = original.clone();
    extra["transition"]["authority"] = json!("invented");
    assert!(
        restore_bank_expiry(
            &source,
            binding(&source, viewer_b()),
            &serde_json::to_vec(&extra).unwrap()
        )
        .is_err()
    );
    assert!(restore_input(&source, binding(&source, viewer_b()), prepared.record()).is_err());
    assert!(restore_entropy(&source, binding(&source, viewer_b()), prepared.record()).is_err());
    assert!(
        restore_bank_expiry(
            restored.state(),
            binding(restored.state(), viewer_b()),
            prepared.record()
        )
        .is_err()
    );
    let player = scope(&source, viewer_a());
    let command = command(&player, PlayerAction::Pass {});
    let proposal = player.propose(&command).unwrap();
    let player_bytes = player.encode_input(&proposal).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&player_bytes).unwrap()["version"],
        "native-input-v1"
    );
    assert!(restore_bank_expiry(&source, binding(&source, viewer_a()), &player_bytes).is_err());
    assert_eq!(source, before);
}
