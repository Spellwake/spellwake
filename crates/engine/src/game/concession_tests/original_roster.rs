//! Original-format guards. Replacing execution below simulates a later live cut;
//! it does not implement or qualify an adoptable player-departure transition.
use super::*;

#[test]
fn original_order_survives_actual_ordinary_candidates() {
    let original = vec![C, A, B];
    let state = root(vec![], &[20, 20, 20], None, original.clone());
    assert_eq!(state.original_participants(), original);
    let candidate = state.advance(input(&state, A, GameCommand::Pass)).unwrap();
    assert_eq!(candidate.state().original_participants(), original);
    assert_eq!(state.original_participants(), original);
    assert_eq!(
        candidate.state().execution.window().participants(),
        original
    );
}

#[test]
fn original_multiplayer_scope_survives_actual_three_to_two_without_authorizing_the_removed_seat() {
    let initial = root(vec![], &[20, 20, 20], None, vec![C, A, B]);
    let state = initial
        .advance(input(&initial, C, GameCommand::Concede))
        .unwrap()
        .into_state();
    let before = state.clone();
    assert_eq!(state.original_participants(), &[C, A, B]);
    assert_eq!(state.execution.window().participants(), &[A, B]);
    assert_eq!(
        state.advance(input(&state, C, GameCommand::Pass)),
        Err(GameError::UnknownActor)
    );
    let final_concession = state
        .advance(input(&state, A, GameCommand::Concede))
        .unwrap();
    assert_eq!(
        final_concession.state().outcome(),
        Some(GameOutcome::Winner(B))
    );
    assert_eq!(
        final_concession.state().conclusion().unwrap().cause(),
        &GameConclusionCause::ConcededLastSurvivor {
            player: A,
            earlier_departures: BTreeSet::from([C])
        }
    );
    assert_eq!(
        state.expire_bank(BankExpiryInput {
            basis: state.basis.clone(),
            player: A
        }),
        Err(GameError::UnsupportedBankExpiryScope)
    );
    assert_eq!(state, before);
    let survivor = state.advance(input(&state, A, GameCommand::Pass)).unwrap();
    assert_eq!(survivor.state().original_participants(), &[C, A, B]);
}

#[test]
fn original_multiplayer_checked_losses_do_not_acquire_two_player_terminal_semantics() {
    let mut state = root(vec![], &[20, 20, 20], None, vec![A, B, C]);
    let live = root(vec![], &[0, 20], None, vec![A, B]);
    assert!(
        live.outcome().is_some(),
        "ordinary two-player checked losses still conclude"
    );
    state.execution = live.execution;
    state.finalize_checked_losses();
    assert_eq!(state.outcome(), None);
    assert_eq!(state.pending_eliminations(), vec![A]);
    assert_eq!(
        state.advance(input(&state, B, GameCommand::Pass)),
        Err(GameError::PlayerEliminationRequired)
    );
}

#[test]
fn completed_opening_uses_original_format_for_first_draw_after_simulated_live_reduction() {
    for original in [vec![A, B], vec![A, B, C]] {
        let lives = vec![20; original.len()];
        let mut state = root(vec![], &lives, None, original.clone());
        let (_, live_seed) = seed(vec![], vec![A, B], &[20, 20]);
        let execution = EffectWindow::new(live_seed).unwrap();
        state.execution = execution.begin_opening(execution.revision()).unwrap();
        let mut opening = OpeningState::new(original.len() > 2);
        opening.stage = OpeningStage::Complete;
        state.opening = Some(opening);
        state.pending = None;
        state
            .drive_opening(&mut TransitionFacts::default())
            .unwrap();
        assert_eq!(
            state.turn.as_ref(),
            Some(&TurnScheduler::new(TurnStart::default(), original.len() == 2).unwrap())
        );
        assert_eq!(state.original_participants(), original);
    }
}

#[test]
fn failed_draw_settles_the_match_after_an_actual_multiplayer_timeout() {
    let before = root(vec![], &[20, 20, 20], Some(Step::Upkeep), vec![A, B, C]);
    let timed_out = before
        .expire_response(before.basis(), A)
        .unwrap()
        .into_state();
    assert_eq!(timed_out.execution.window().participants(), &[B, C]);
    let mut next = timed_out;
    for _ in 0..50 {
        if let Some(conclusion) = next.conclusion() {
            assert_eq!(conclusion.outcome(), GameOutcome::Winner(C));
            assert_eq!(
                conclusion.losing_players().collect::<BTreeSet<_>>(),
                BTreeSet::from([A, B])
            );
            assert_eq!(next.execution.state_based_losses(), &BTreeSet::from([B]));
            assert_eq!(before.execution.window().participants(), &[A, B, C]);
            return;
        }
        let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
            next.execution.window().phase()
        else {
            panic!(
                "expected a survivor to receive priority: {:?}",
                next.execution.window().phase()
            );
        };
        next = next
            .advance(input(&next, holder, GameCommand::Pass))
            .unwrap()
            .into_state();
    }
    panic!("surviving player's empty draw did not settle the game");
}
