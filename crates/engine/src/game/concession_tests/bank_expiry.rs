//! Trusted bank transition mechanics only; SQL time/authority and multiplayer are separate.
use super::*;

fn expire(state: &GameState, player: SeatId) -> NativeCandidate {
    let before = state.clone();
    let request = BankExpiryInput {
        basis: state.basis().clone(),
        player,
    };
    let candidate = state.expire_bank(request.clone());
    assert_eq!(candidate, state.expire_bank(request));
    assert_eq!(*state, before);
    let candidate = candidate.unwrap();
    let terminal = candidate.state();
    assert_eq!(
        candidate.base(),
        &CandidateBase::Advance(before.basis().clone())
    );
    assert_eq!(terminal.basis.revision, before.basis.revision + 1);
    assert_eq!(
        terminal.outcome(),
        Some(GameOutcome::Winner(if player == A { B } else { A }))
    );
    let conclusion = terminal.conclusion().unwrap();
    assert_eq!(
        conclusion.cause(),
        &GameConclusionCause::BankExpired { player }
    );
    assert_eq!(
        conclusion.losing_players().collect::<Vec<_>>(),
        vec![player]
    );
    assert_eq!(
        candidate.rules_journal().records,
        vec![RulesRecord::GameConcluded {
            conclusion: conclusion.clone()
        }]
    );
    let mut frozen = terminal.clone();
    frozen.basis = before.basis.clone();
    frozen.conclusion = before.conclusion.clone();
    assert_eq!(
        frozen, before,
        "expiry freezes existing resources, work and accepted prefix"
    );
    assert!(terminal.pending_decision().is_none());
    assert!(terminal.pending_turn_work().is_none());
    assert_eq!(
        terminal.retained_terminal_decision(),
        before.pending.as_ref()
    );
    assert_eq!(
        terminal.advance(input(terminal, player, GameCommand::Pass)),
        Err(GameError::GameEnded)
    );
    assert_eq!(
        terminal.supply_entropy(terminal.basis(), vec![0]),
        Err(GameError::GameEnded)
    );
    candidate
}

#[test]
fn bank_expiry_follows_actual_priority_not_an_arbitrary_participant() {
    let state = root(vec![], &[20, 20], None, vec![A, B]);
    expire(&state, A);
    let before = state.clone();
    assert_eq!(
        state.expire_bank(BankExpiryInput {
            basis: state.basis().clone(),
            player: B
        }),
        Err(GameError::BankPlayerNotWaiting)
    );
    assert_eq!(state, before);
    let passed = step(&state, A, GameCommand::Pass);
    expire(&passed, B);
    assert_eq!(
        passed.expire_bank(BankExpiryInput {
            basis: passed.basis().clone(),
            player: A
        }),
        Err(GameError::BankPlayerNotWaiting)
    );
}

#[test]
fn bank_expiry_preserves_actual_opening_decision_and_entropy() {
    let cards = [A, B]
        .into_iter()
        .flat_map(|owner| {
            (0..12).map(move |n| {
                card(
                    100 + u64::from(owner.0) * 100 + n,
                    owner,
                    ObjectDestination::LibraryBottom,
                    Program::artifact(None, vec![]),
                )
            })
        })
        .collect();
    let (context, seed) = seed(cards, vec![A, B], &[20, 20]);
    let state = GameState::initialize_opening(
        GameId(1044),
        context,
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 22]),
        },
    )
    .unwrap()
    .into_state();
    let chooser = state.pending_decision().unwrap().chooser();
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Opening(_)
    ));
    expire(&state, chooser);
    let other = if chooser == A { B } else { A };
    assert_eq!(
        state.expire_bank(BankExpiryInput {
            basis: state.basis().clone(),
            player: other
        }),
        Err(GameError::BankPlayerNotWaiting)
    );
}

#[test]
fn bank_expiry_uses_the_issued_effect_chooser_without_executing_the_choice() {
    let state = resolve(
        fixture(
            vec![
                Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::TargetPlayer,
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                },
            ],
            BodyTarget::SinglePlayer,
            vec![],
        ),
        Some(TargetRef::Player(B)),
    );
    assert_eq!(state.pending_decision().unwrap().chooser(), B);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    assert_eq!(
        state.expire_bank(BankExpiryInput {
            basis: state.basis().clone(),
            player: A
        }),
        Err(GameError::BankPlayerNotWaiting)
    );
    expire(&state, B);
}

#[test]
fn bank_expiry_rejections_preserve_state_and_error_precedence() {
    let state = root(vec![], &[20, 20], None, vec![A, B]);
    let reject = |source: &GameState, request: BankExpiryInput, expected| {
        let before = source.clone();
        assert_eq!(source.expire_bank(request.clone()), Err(expected));
        assert_eq!(
            source.expire_bank(request.clone()),
            source.expire_bank(request)
        );
        assert_eq!(*source, before);
    };
    let request = BankExpiryInput {
        basis: state.basis().clone(),
        player: A,
    };
    let mut bad = request.clone();
    bad.basis.game = GameId(999);
    reject(&state, bad, GameError::WrongGame);
    let mut bad = request.clone();
    bad.basis.context = GameContext::new(
        "other".into(),
        state.basis.context.rules().into(),
        "c".into(),
        "p".into(),
        "p".into(),
        state.basis.context.creature_vocabulary_id(),
        state.basis.context.name_rules_id(),
    )
    .unwrap();
    reject(&state, bad, GameError::WrongContext);
    let mut bad = request.clone();
    bad.basis.epoch += 1;
    reject(&state, bad, GameError::WrongEpoch);
    let mut bad = request.clone();
    bad.basis.revision += 1;
    reject(&state, bad, GameError::StaleRevision);
    let mut bad = request.clone();
    bad.player = C;
    reject(&state, bad, GameError::UnknownActor);
    let terminal = expire(&state, A).into_state();
    reject(&terminal, request, GameError::StaleRevision);
    reject(
        &terminal,
        BankExpiryInput {
            basis: terminal.basis().clone(),
            player: A,
        },
        GameError::GameEnded,
    );
    let mut exhausted = state.clone();
    exhausted.basis.revision = u64::MAX;
    reject(
        &exhausted,
        BankExpiryInput {
            basis: exhausted.basis().clone(),
            player: A,
        },
        GameError::RevisionExhausted,
    );
    let multi = root(vec![], &[20, 20, 20], None, vec![A, B, C]);
    reject(
        &multi,
        BankExpiryInput {
            basis: multi.basis().clone(),
            player: A,
        },
        GameError::UnsupportedBankExpiryScope,
    );
}

#[test]
fn bank_expiry_cannot_be_projected_as_an_old_reference_root() {
    use crate::test_support::{
        current_profile::applicability::{ApplicabilityError, NativeCaptureValue},
        packet::AliasContext,
        projection,
    };
    let source = root(vec![], &[20, 20], None, vec![A, B]);
    let candidate = expire(&source, A);
    assert_eq!(candidate.state().check_current10(), Err(ApplicabilityError));
    assert!(projection::state(candidate.state(), &AliasContext::default()).is_err());
    assert!(projection::candidate(&candidate, Some(&source), &AliasContext::default()).is_err());
    assert_eq!(
        candidate.rules_journal().check_current10(),
        Err(ApplicabilityError)
    );
}

#[test]
fn bank_expiry_refuses_a_real_completed_priority_round_without_a_waiter() {
    let initial = root(vec![], &[20, 20], None, vec![A, B]);
    let passed = step(&initial, A, GameCommand::Pass);
    let ended = step(&passed, B, GameCommand::Pass);
    assert!(ended.pending_decision().is_none());
    assert!(ended.pending_turn_work().is_none());
    assert!(ended.execution().pending_work_kind().is_none());
    assert_eq!(
        ended.execution().window().phase(),
        crate::stack_window::WindowPhase::StepEnded
    );
    for player in [A, B] {
        let before = ended.clone();
        let input = BankExpiryInput {
            basis: ended.basis().clone(),
            player,
        };
        assert_eq!(
            ended.expire_bank(input.clone()),
            Err(GameError::BankPlayerNotWaiting)
        );
        assert_eq!(ended.expire_bank(input.clone()), ended.expire_bank(input));
        assert_eq!(ended, before);
    }
}

#[test]
fn bank_expiry_refuses_a_real_scheduled_combat_work_cut_without_a_choice() {
    let state = root(vec![], &[20, 20], Some(Step::DeclareBlockers), vec![A, B]);
    assert!(state.pending_decision().is_none());
    assert_eq!(state.pending_turn_work(), Some(TurnWork::DeclareBlockers));
    assert_eq!(
        state.execution().window().phase(),
        crate::stack_window::WindowPhase::TurnWork
    );
    let before = state.clone();
    for player in [A, B] {
        assert_eq!(
            state.expire_bank(BankExpiryInput {
                basis: state.basis().clone(),
                player
            }),
            Err(GameError::BankPlayerNotWaiting)
        );
        assert_eq!(state, before);
    }
}
