//! Genuine source/opening chain; Storage is a strict byte-read mock here.
//! Actual SQL/private Storage acceptance is qualified separately.
use super::*;
use spellwake_engine::{
    priority::PriorityPhase, stack_window::WindowPhase, turn::Step as TurnStep,
};
fn advance(f: &mut CheckpointFixture, viewer: Uuid, action: PlayerAction) {
    let scope = NativeScope::new(f.tip.state(), f.tip.binding(viewer).unwrap()).unwrap();
    if matches!(action, PlayerAction::PlayLand { .. }) {
        unreachable!("land uses actual issued object")
    }
    let command = PlayerCommand {
        request_id: Uuid::new_v4(),
        basis: scope.basis().clone(),
        action,
    };
    let proposal = scope.propose(&command).unwrap();
    let bytes = scope.encode_input(&proposal).unwrap();
    install(f, viewer, bytes);
}
pub(super) fn install(f: &mut CheckpointFixture, viewer: Uuid, bytes: Vec<u8>) {
    let input = NativeArtifactRef::for_bytes(
        ids().game_id,
        ids().context_id,
        NativeArtifactRole::Input,
        &bytes,
    )
    .unwrap();
    let prior_counts: Vec<_> = seats()
        .iter()
        .map(|seat| f.tip.own_turns(*seat).unwrap())
        .collect();
    let next = with_store(&bytes, &input, |store| {
        f.tip
            .append(store, input.clone(), NativeCheckpointInput::Player, viewer)
    })
    .unwrap();
    assert_eq!(
        prior_counts,
        seats()
            .iter()
            .map(|seat| f.tip.own_turns(*seat).unwrap())
            .collect::<Vec<_>>()
    );
    f.inputs.push((input, bytes));
    f.states
        .push((next.reference().clone(), next.original_bytes().to_vec()));
    f.candidates.push(next.candidate().clone());
    f.tip = next;
}
pub(super) fn replay(f: &CheckpointFixture) -> NativeCheckpoint {
    let initial = reference(&f.original);
    let mut steps = vec![];
    for (r, b) in f.states.iter().rev() {
        steps.extend(checkpoint_read_steps(r, b));
    }
    steps.extend(checkpoint_read_steps(&initial, &f.original));
    for (r, b) in &f.inputs {
        steps.extend(checkpoint_read_steps(r, b));
    }
    let mock = Mock::new(steps);
    let recovered = load_checkpoint(
        &checkpoint_store(&mock),
        &NativeCheckpointExpected::new(f.tip.reference().clone(), f.tip.basis().clone()).unwrap(),
        &expected(initial),
        source(NativeProgramPool::ResolutionSelectionNaturalMana),
        NativeRecoveryBudget::new(f.states.len(), 100_000_000).unwrap(),
    )
    .unwrap();
    mock.finish();
    recovered
}
#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary; native producer mechanism, not admission"]
fn genuine_turn_starts_and_terminal_survive_exact_cold_replay() {
    let mut f = checkpoint_fixture(None);
    assert_eq!(f.tip.own_turns(seats()[0]), Some(0));
    assert_eq!(f.tip.own_turns(seats()[1]), Some(1));
    assert_eq!(f.tip.own_turns(Uuid::new_v4()), None);
    let baseline = f.tip.prepare_results().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(baseline.original_bytes()).unwrap()["terminal"],
        Value::Null
    );
    for _ in 0..400 {
        if f.tip.own_turns(seats()[1]) == Some(3) {
            break;
        }
        assert!(f.tip.state().pending_decision().is_none());
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            f.tip.state().execution().window().phase()
        else {
            panic!("expected actual priority")
        };
        let viewer = seats()[usize::try_from(holder.0).unwrap()];
        let turn = f.tip.state().turn().unwrap();
        let active = f.tip.state().execution().window().turn_owner();
        if holder == active
            && turn.position().step == TurnStep::PrecombatMain
            && turn.lands_played() == &0.into()
        {
            let land = f
                .tip
                .state()
                .execution()
                .window()
                .cards()
                .iter()
                .find(|object| {
                    object.owner() == holder
                        && object.zone()
                            == spellwake_engine::object_identity::ObjectZone::Hand(holder)
                })
                .unwrap()
                .id();
            let mut scope =
                NativeScope::new(f.tip.state(), f.tip.binding(viewer).unwrap()).unwrap();
            let handle = scope.issue(NativeReference::Object(land)).unwrap();
            let proposal = scope
                .propose(&PlayerCommand {
                    request_id: Uuid::new_v4(),
                    basis: scope.basis().clone(),
                    action: PlayerAction::PlayLand { source: handle },
                })
                .unwrap();
            let bytes = scope.encode_input(&proposal).unwrap();
            install(&mut f, viewer, bytes);
        } else {
            advance(&mut f, viewer, PlayerAction::Pass {});
        }
    }
    assert_eq!(f.tip.own_turns(seats()[1]), Some(3));
    assert_eq!(f.tip.own_turns(seats()[0]), Some(2));
    let recovered = replay(&f);
    let prior_event = f.tip.prepare_event().unwrap();
    assert_eq!(
        recovered.prepare_event().unwrap().original_bytes(),
        prior_event.original_bytes()
    );
    assert!(recovered.prepare_event().unwrap().reference() == prior_event.reference());
    assert_eq!(
        recovered.prepare_results().unwrap().original_bytes(),
        f.tip.prepare_results().unwrap().original_bytes()
    );
    // Conceding during the third own turn preserves that start credit.
    advance(&mut f, seats()[1], PlayerAction::Concede {});
    let terminal = f.tip.terminal().unwrap().unwrap();
    assert_eq!(terminal.winner_seat_ids(), &[seats()[0]]);
    assert_eq!(terminal.losing_seat_ids(), &[seats()[1]]);
    assert_eq!(terminal.reason(), "native_concession");
    assert_eq!(terminal.basis(), f.tip.basis());
    assert_eq!(f.tip.own_turns(seats()[1]), Some(3));
    let restored = replay(&f);
    let terminal_event = f.tip.prepare_event().unwrap();
    assert!(terminal_event.reference() != prior_event.reference());
    assert_eq!(
        restored.prepare_event().unwrap().original_bytes(),
        terminal_event.original_bytes()
    );
    assert_eq!(
        restored.prepare_results().unwrap().original_bytes(),
        f.tip.prepare_results().unwrap().original_bytes()
    );
    assert_eq!(restored.own_turns(seats()[0]), Some(2));
    assert_eq!(restored.own_turns(seats()[1]), Some(3));
}
#[test]
#[ignore = "requires explicit genuine catalogue/vocabulary; no durable acceptance claim"]
fn actual_first_turn_concession_retains_below_threshold_counts() {
    let mut f = checkpoint_fixture(None);
    advance(&mut f, seats()[0], PlayerAction::Concede {});
    let restored = replay(&f);
    assert_eq!(restored.own_turns(seats()[0]), Some(0));
    assert_eq!(restored.own_turns(seats()[1]), Some(1));
    assert_eq!(
        restored.terminal().unwrap().unwrap().winner_seat_ids(),
        &[seats()[1]]
    );
    let evidence: Value =
        serde_json::from_slice(restored.prepare_results().unwrap().original_bytes()).unwrap();
    assert_eq!(evidence["turnStarts"][0]["ownTurns"], "0");
    assert_eq!(evidence["turnStarts"][1]["ownTurns"], "1");
}
