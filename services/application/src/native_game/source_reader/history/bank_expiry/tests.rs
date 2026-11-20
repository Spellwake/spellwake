//! Genuine native cut plus controlled SQL-source metadata and strict Storage mocks.
//! These tests bind provenance; they do not authenticate a real SQL timeout.
use super::*;
use crate::native_game::initial_source::tests::{
    checkpoint_fixture, checkpoint_read_steps, checkpoint_store,
};
use crate::test_http::Mock;
use serde_json::{Value, json};

mod commit;
mod state_only;
fn cut() -> (NativeCheckpoint, Value) {
    let (checkpoint, evidence, _) = cut_from(&checkpoint_fixture(None));
    (checkpoint, evidence)
}
fn cut_from(
    f: &crate::native_game::initial_source::tests::CheckpointFixture,
) -> (NativeCheckpoint, Value, Vec<u8>) {
    let viewer = f.tip.roster[0].seat_id;
    let binding = f.tip.binding(viewer).unwrap();
    let holder = match f.tip.state().execution().window().phase() {
        spellwake_engine::stack_window::WindowPhase::Priority(
            spellwake_engine::priority::PriorityPhase::Ready { holder },
        ) => holder,
        _ => panic!("genuine fixture must expose actual priority"),
    };
    let loser = binding
        .seats
        .iter()
        .find(|seat| seat.native_seat == holder)
        .unwrap()
        .seat_id;
    let prepared = NativeScope::new(f.tip.state(), binding)
        .unwrap()
        .prepare_bank_expiry(loser)
        .unwrap();
    let input = NativeArtifactRef::for_bytes(
        f.tip.basis().game_id,
        f.tip.basis().context_id,
        NativeArtifactRole::Input,
        prepared.record(),
    )
    .unwrap();
    let original = json!({"version":"native-host-input-v1","basis":f.tip.basis(),"kind":"clock","inputRef":input,"runtimeId":native_runtime_identity(),"programSourceId":crate::native_game::initial_source::tests::source(NativeProgramPool::ResolutionSelectionNaturalMana).identity().hex()});
    let mock = Mock::new(checkpoint_read_steps(&input, prepared.record()));
    let next = f
        .tip
        .append(
            &checkpoint_store(&mock),
            input.clone(),
            NativeCheckpointInput::BankExpiry,
            viewer,
        )
        .unwrap();
    mock.finish();
    assert_eq!(next.candidate(), prepared.candidate());
    let evidence = json!({"originalRequestId":Uuid::from_u128(0xbeef),"original":original,
        "timeout":{"kind":"overall_bank","seatId":loser,"clockRevision":"4","proofRef":input},
        "beforeClock":{"seatId":loser,"remainingMs":"1","clockRevision":"4","paused":false,"lastAccountedAt":"2026-11-16T00:00:00.000500Z"},
        "acceptedAt":"2026-11-16T00:00:00.001400Z"});
    (next, evidence, prepared.record().to_vec())
}
fn parse(value: &Value) -> BankExpiryEvidence {
    crate::decode_wire_object(&serde_json::to_vec(value).unwrap()).unwrap()
}
fn selected(
    checkpoint: &NativeCheckpoint,
    evidence: Option<Value>,
    marker: Value,
) -> NativeTransitionSources {
    let basis = checkpoint.basis();
    let creator = Uuid::from_u128(0x91);
    let id = Uuid::from_u128(0x92);
    let mut group = json!({"checkpointId":id,"acceptedRevision":basis.revision,"stateRef":checkpoint.reference(),
        "factsRef":checkpoint.prepare_results().unwrap().reference(),"creatorReceiptId":creator,"eventSchemaRevision":marker,
        "events":[{"eventId":Uuid::from_u128(0x93),"sequence":basis.revision,"acceptedRevision":basis.revision,
        "privatePayloadRef":checkpoint.prepare_event().unwrap().reference(),"acceptedAt":"2026-11-16T00:00:00.001500Z","creatorReceiptId":creator,"facts":[]}]});
    if let Some(value) = evidence {
        group["bankExpiry"] = value;
    }
    let page: super::super::Page = crate::decode_wire_object(&serde_json::to_vec(&json!({
        "anchor":{"gameId":basis.game_id,"contextId":basis.context_id,"anchorCheckpointId":id,
        "anchorStateRef":checkpoint.reference(),"anchorCreatorReceiptId":creator,"throughAcceptedRevision":basis.revision,"throughEventSequence":basis.revision},
        "groups":[group],"nextAfterRevision":null})).unwrap()).unwrap();
    NativeTransitionSources {
        anchor: page.anchor,
        groups: page.groups,
        cast_accounts: crate::native_game::cast_activity::NativeCastAccounts::new(
            basis.game_id,
            basis.context_id,
            checkpoint
                .roster
                .iter()
                .map(|seat| (seat.seat_id, Some(seat.seat_id))),
        )
        .unwrap(),
    }
}
fn rejects_before_io(checkpoint: &NativeCheckpoint, selected: &NativeTransitionSources) {
    let mock = Mock::new(vec![]);
    assert!(
        super::super::super::history_replay::HistoryReplay::new(selected)
            .observe(
                &checkpoint_store(&mock),
                checkpoint,
                &mut NativeRecoveryBudget::new(20, 1_000_000).unwrap()
            )
            .is_err()
    );
    assert!(mock.finish().is_empty());
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; bank source binding with mocked Storage only"]
fn bank_history_evidence_matches_actual_input_and_rejects_foreign_or_early_sources_before_io() {
    let (checkpoint, good) = cut();
    let before = checkpoint.state().clone();
    parse(&good).verify(&checkpoint).unwrap();
    let mut zero = good.clone();
    zero["beforeClock"]["remainingMs"] = json!("0");
    zero["beforeClock"]["paused"] = json!(true);
    zero["acceptedAt"] = zero["beforeClock"]["lastAccountedAt"].clone();
    parse(&zero).verify(&checkpoint).unwrap(); // SQL separately authenticates inherited exhaustion.
    let other = checkpoint.terminal().unwrap().unwrap().winner_seat_ids()[0];
    let foreign = NativeArtifactRef::for_bytes(
        checkpoint.basis().game_id,
        checkpoint.basis().context_id,
        NativeArtifactRole::Input,
        b"foreign input",
    )
    .unwrap();
    for (pointer, value) in [
        ("/original/kind", json!("entropy")),
        ("/original/kind", json!("native-work")),
        (
            "/original/basis/revision",
            json!(checkpoint.basis().revision),
        ),
        ("/original/basis/epoch", json!("999")),
        ("/original/inputRef", json!(foreign)),
        ("/timeout/kind", json!("other")),
        ("/timeout/seatId", json!(other)),
        ("/timeout/clockRevision", json!("5")),
        ("/timeout/proofRef", json!(foreign)),
        ("/beforeClock/seatId", json!(other)),
        ("/beforeClock/clockRevision", json!("5")),
        ("/beforeClock/remainingMs", json!("2")),
        ("/beforeClock/paused", json!(true)),
        (
            "/beforeClock/lastAccountedAt",
            json!("2026-11-16T00:00:00.002000Z"),
        ),
        ("/acceptedAt", json!("2026-11-16T00:00:00.000999Z")),
        ("/acceptedAt", json!("invalid")),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(
            parse(&bad).verify(&checkpoint).is_err(),
            "accepted corruption at {pointer}"
        );
        rejects_before_io(
            &checkpoint,
            &selected(&checkpoint, Some(bad), json!("native-transition-v1")),
        );
    }
    rejects_before_io(
        &checkpoint,
        &selected(&checkpoint, None, json!("native-transition-v1")),
    );
    rejects_before_io(
        &checkpoint,
        &selected(&checkpoint, Some(good.clone()), Value::Null),
    );
    let ordinary = checkpoint_fixture(None).tip;
    rejects_before_io(
        &ordinary,
        &selected(&ordinary, Some(good), json!("native-transition-v1")),
    );
    assert_eq!(checkpoint.state(), &before);
}
#[test]
#[ignore = "requires genuine catalogue/vocabulary; exact bank Event/result mock read only"]
fn bank_history_verifies_exact_event_results_after_source_binding() {
    let (checkpoint, good) = cut();
    let selected = selected(&checkpoint, Some(good), json!("native-transition-v1"));
    let event = checkpoint.prepare_event().unwrap();
    let results = checkpoint.prepare_results().unwrap();
    let mut steps = checkpoint_read_steps(event.reference(), event.original_bytes());
    steps.extend(checkpoint_read_steps(
        results.reference(),
        results.original_bytes(),
    ));
    let mock = Mock::new(steps);
    let mut budget =
        NativeRecoveryBudget::new(2, event.reference().size() + results.reference().size())
            .unwrap();
    let mut verifier = super::super::super::history_replay::HistoryReplay::new(&selected);
    verifier
        .observe(&checkpoint_store(&mock), &checkpoint, &mut budget)
        .unwrap();
    assert_eq!(
        verifier.finish().unwrap(),
        NativeHistoryEvidence::TransitionV1
    );
    assert_eq!(budget.remaining_bytes(), 0);
    assert_eq!(mock.finish().len(), 4);
}
