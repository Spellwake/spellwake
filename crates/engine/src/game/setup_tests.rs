//! Candidate rollback at and after the private actual turn-start operation.
use super::*;
use crate::combat::CombatRules;
use crate::{
    effects::Program,
    mana::{FloatingMana, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectIdentityError},
    setup::GameSeed,
    stack_window::StackWindowError,
    turn::Step,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn context() -> GameContext {
    GameContext::new(
        "r".into(),
        "cr".into(),
        "cat".into(),
        "p".into(),
        "control".into(),
        crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr").id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}

fn creature(id: u64, owner: SeatId, destination: ObjectDestination) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination,
        program: Program::creature(None, 2.into(), 3.into(), std::collections::BTreeSet::new()),
    }
}

fn pass(state: &GameState, actor: SeatId) -> GameInput {
    GameInput {
        basis: state.basis.clone(),
        actor,
        command: GameCommand::Pass,
    }
}

fn end_after_first_pass() -> GameState {
    let state = GameState::initialize_scheduled(
        GameId(30026),
        context(),
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            vec![
                creature(1, A, ObjectDestination::Battlefield { controller: A }),
                creature(2, B, ObjectDestination::Battlefield { controller: B }),
            ],
            vec![CardId(1), CardId(2)],
            vec![
                FloatingMana {
                    player: A,
                    mana_type: ManaType::Green,
                    amount: 7.into(),
                },
                FloatingMana {
                    player: B,
                    mana_type: ManaType::Blue,
                    amount: 11.into(),
                },
            ],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        ),
        TurnStart {
            number: 8,
            step: Step::End,
        },
    )
    .unwrap()
    .into_state();
    state.advance(pass(&state, A)).unwrap().into_state()
}

#[test]
fn failed_new_turn_preserves_the_accepted_pass_resources_life_and_unstamped_cards() {
    for fault in 0..5 {
        let mut state = end_after_first_pass();
        // End pass, Cleanup entry/work, then Untap entry precede the stamp.
        // The second offset at each enclosing owner allows the stamp to succeed
        // on the candidate before the following untap operation fails.
        let expected = match fault {
            0 => {
                state.execution.set_test_object_allocator(u64::MAX, false);
                GameError::Effect(EffectError::Window(StackWindowError::Objects(
                    ObjectIdentityError::RevisionExhausted,
                )))
            }
            1 | 2 => {
                state
                    .execution
                    .set_test_window_revision(u64::MAX - if fault == 1 { 3 } else { 4 });
                GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
            }
            _ => {
                state
                    .execution
                    .set_test_revision(u64::MAX - if fault == 3 { 4 } else { 5 });
                GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
            }
        };
        let before = state.clone();
        assert_eq!(
            state.advance(pass(&state, B)),
            Err(expected),
            "fault {fault}"
        );
        assert_eq!(state, before);
        assert_eq!(state.turn().unwrap().position().step, Step::End);
        assert_eq!(state.turn().unwrap().position().number, 8);
        assert_eq!(state.execution.window().turn_owner(), A);
        assert_eq!(state.execution.mana(A, ManaType::Green), 7.into());
        assert_eq!(state.execution.mana(B, ManaType::Blue), 11.into());
        for player in [A, B] {
            assert_eq!(state.execution.life_total(player), Some(&20.into()));
        }
        assert!(
            state
                .execution
                .window()
                .cards()
                .iter()
                .all(|card| { card.has_continuous_control_since_turn_start() == Some(false) })
        );
    }
}

fn opening_after_first_keep() -> GameState {
    let mut cards = vec![];
    for (owner, base) in [(A, 100), (B, 200)] {
        cards.extend(
            (0..12).map(|index| creature(base + index, owner, ObjectDestination::LibraryBottom)),
        );
    }
    let state = GameState::initialize_opening(
        GameId(1033026),
        context(),
        OpeningSetup {
            seed: GameSeed::with_uniform_life(
                vec![A, B],
                A,
                cards,
                vec![],
                vec![],
                20.into(),
                CombatRules::UnrestrictedFreeForAll,
                crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr"),
                std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
            ),
            entropy: EntropyTape::new(vec![0; 1000]),
        },
    )
    .unwrap()
    .into_state();
    state.advance(keep(&state)).unwrap().into_state()
}

fn keep(state: &GameState) -> GameInput {
    let decision = state.pending_decision().unwrap();
    GameInput {
        basis: state.basis.clone(),
        actor: decision.chooser(),
        command: GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::Mulligan(MulliganChoice::Keep),
        },
    }
}

#[test]
fn failed_opening_turn_start_retains_the_prior_vote_and_both_accepted_hands() {
    for fault in 0..5 {
        let mut state = opening_after_first_keep();
        assert_eq!(state.pending_decision().unwrap().chooser(), B);
        let expected = match fault {
            0 => {
                state.execution.set_test_object_allocator(u64::MAX, false);
                GameError::Effect(EffectError::Window(StackWindowError::Objects(
                    ObjectIdentityError::RevisionExhausted,
                )))
            }
            1 | 2 => {
                state
                    .execution
                    .set_test_window_revision(u64::MAX - if fault == 1 { 1 } else { 2 });
                GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
            }
            _ => {
                state
                    .execution
                    .set_test_revision(u64::MAX - if fault == 3 { 1 } else { 2 });
                GameError::Effect(EffectError::Window(StackWindowError::RevisionExhausted))
            }
        };
        let before = state.clone();
        assert_eq!(state.advance(keep(&state)), Err(expected), "fault {fault}");
        assert_eq!(state, before);
        assert!(state.turn().is_none());
        assert_eq!(state.execution.window().phase(), WindowPhase::Opening);
        for player in [A, B] {
            assert_eq!(
                state
                    .execution
                    .window()
                    .cards()
                    .iter()
                    .filter(|card| card.zone() == ObjectZone::Hand(player))
                    .count(),
                7
            );
            assert_eq!(state.execution.life_total(player), Some(&20.into()));
        }
        assert_eq!(state.pending_decision(), before.pending_decision());
        assert_eq!(state.entropy(), before.entropy());
    }
}
