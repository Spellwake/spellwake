//! Explicit native setup and ordinary life-loss boundaries; not format qualification.
use spellwake_contracts::RulesInteger;
use spellwake_engine::combat::CombatRules;
use spellwake_engine::{
    draw::GameOutcome,
    effects::{DecisionReply, EffectError, EffectWindow, Program, SeedCard},
    game::{GameCommand, GameContext, GameError, GameId, GameInput, GameState, OpeningSetup},
    mana::{FloatingMana, ManaType},
    object_identity::{CardId, ObjectDestination, ObjectZone},
    opening::{MulliganChoice, OpeningError},
    priority::SeatId,
    setup::{GameSeed, StartingLife},
    shuffle::EntropyTape,
    stack_window::WindowPhase,
    turn::{Step, TurnStart},
};
const A: SeatId = SeatId(17);
const B: SeatId = SeatId(4);
const C: SeatId = SeatId(91);

fn context() -> GameContext {
    GameContext::new(
        "setup".into(),
        "CR20260925".into(),
        "authored".into(),
        "literal".into(),
        "fixture".into(),
        test_vocabulary().id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap()
}
fn seed() -> GameSeed {
    GameSeed::with_uniform_life(
        vec![A, B],
        A,
        vec![],
        vec![],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        test_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    )
}
fn total(player: SeatId, value: i64) -> StartingLife {
    StartingLife {
        player,
        total: value.into(),
    }
}
fn opening_seed() -> GameSeed {
    let mut input = seed();
    input.cards = [A, B]
        .into_iter()
        .enumerate()
        .flat_map(|(index, owner)| {
            (0..8).map(move |n| SeedCard {
                card: CardId((index * 100 + n) as u64),
                owner,
                destination: ObjectDestination::LibraryBottom,
                program: Program::creature(
                    None,
                    1.into(),
                    1.into(),
                    std::collections::BTreeSet::new(),
                ),
            })
        })
        .collect();
    input
}
fn keep(game: &GameState) -> GameState {
    let pending = game.pending_decision().unwrap();
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor: pending.chooser(),
        command: GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::Mulligan(MulliganChoice::Keep),
        },
    })
    .unwrap()
    .into_state()
}

#[test]
fn explicit_nonuniform_totals_preserve_player_order_and_arbitrary_precision() {
    let huge =
        RulesInteger::try_from("1000000000000000000000000000000000000007".to_string()).unwrap();
    let mut input = seed();
    input.life = vec![
        StartingLife {
            player: B,
            total: huge.clone(),
        },
        total(A, 40),
    ];
    let game = GameState::initialize(GameId(1), context(), input)
        .unwrap()
        .into_state();
    assert_eq!(game.execution().window().participants(), &[A, B]);
    assert_eq!(game.execution().life_total(A), Some(&40.into()));
    assert_eq!(game.execution().life_total(B), Some(&huge));
    assert_eq!(game.execution().life_total(C), None);
    assert_eq!(game.execution().window().turn_owner(), A);
    assert_eq!(game.outcome(), None);
}

#[test]
fn every_life_entry_must_belong_to_exactly_one_participant() {
    for life in [
        vec![],
        vec![total(A, 20)],
        vec![total(A, 20), total(A, 20)],
        vec![total(A, 20), total(C, 20)],
        vec![total(A, 20), total(B, 20), total(C, 20)],
    ] {
        let mut input = seed();
        input.life = life;
        let saved = input.clone();
        assert_eq!(
            EffectWindow::new(input.clone()),
            Err(EffectError::InvalidSetup)
        );
        assert_eq!(
            GameState::initialize(GameId(2), context(), input.clone()),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
        assert_eq!(
            GameState::initialize_scheduled(
                GameId(2),
                context(),
                input.clone(),
                TurnStart::default()
            ),
            Err(GameError::Effect(EffectError::InvalidSetup))
        );
        assert_eq!(input, saved);
    }
}

#[test]
fn uniform_helper_does_not_repair_duplicate_or_unknown_seats() {
    for seats in [vec![A, A], vec![A], vec![B, C]] {
        let input = GameSeed::with_uniform_life(
            seats.clone(),
            A,
            vec![],
            vec![],
            vec![],
            40.into(),
            CombatRules::UnrestrictedFreeForAll,
            test_vocabulary(),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        );
        assert_eq!(input.life.len(), seats.len());
        assert!(EffectWindow::new(input).is_err());
    }
}

#[test]
fn signed_life_is_setup_data_until_the_shared_state_check() {
    let negative =
        RulesInteger::try_from("-999999999999999999999999999999999999999".to_string()).unwrap();
    let mut input = seed();
    input.life = vec![
        total(A, 0),
        StartingLife {
            player: B,
            total: negative.clone(),
        },
    ];
    let state = EffectWindow::new(input).unwrap();
    assert!(state.state_based_losses().is_empty());
    assert_eq!(state.life_total(B), Some(&negative));
    let saved = state.clone();
    let checked = state.settle_checks(state.revision()).unwrap();
    assert_eq!(
        checked
            .state_based_losses()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![B, A]
    );
    assert_eq!(state, saved);
    assert_eq!(checked.life_total(A), Some(&0.into()));
}

#[test]
fn two_player_results_derive_from_all_supported_simultaneous_losses() {
    for (a, b, outcome) in [
        (0, 20, GameOutcome::Winner(B)),
        (20, -7, GameOutcome::Winner(A)),
        (0, -7, GameOutcome::Draw),
    ] {
        let mut input = seed();
        input.life = vec![total(A, a), total(B, b)];
        input.cards.push(SeedCard {
            card: CardId(999),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: Program::creature(None, 1.into(), 0.into(), std::collections::BTreeSet::new()),
        });
        input.initial_battlefield_order = vec![CardId(999)];
        let state = GameState::initialize(GameId(3), context(), input)
            .unwrap()
            .into_state();
        assert_eq!(state.outcome(), Some(outcome));
        let cards = state.execution().window().cards();
        assert_eq!(
            cards
                .get(cards.current_identity(CardId(999)).unwrap())
                .unwrap()
                .zone(),
            ObjectZone::Graveyard(A)
        );
        let saved = state.clone();
        assert_eq!(
            state.advance(GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: GameCommand::Pass
            }),
            Err(GameError::GameEnded)
        );
        assert_eq!(state, saved);
    }
}

#[test]
fn multiplayer_life_loss_eliminates_the_seat_before_survivors_act() {
    let mut input = GameSeed::with_uniform_life(
        vec![A, B, C],
        A,
        vec![],
        vec![],
        vec![],
        40.into(),
        CombatRules::UnrestrictedFreeForAll,
        test_vocabulary(),
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    input.life[1].total = 0.into();
    let game = GameState::initialize(GameId(4), context(), input)
        .unwrap()
        .into_state();
    assert_eq!(game.outcome(), None);
    assert!(game.pending_eliminations().is_empty());
    assert_eq!(game.execution().window().participants(), &[A, C]);
    assert_eq!(
        game.advance(GameInput {
            basis: game.basis().clone(),
            actor: B,
            command: GameCommand::Pass
        }),
        Err(GameError::UnknownActor)
    );
    let pass = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor: A,
            command: GameCommand::Pass,
        })
        .unwrap();
    assert_eq!(
        pass.state().execution().window().phase(),
        WindowPhase::Priority(spellwake_engine::priority::PriorityPhase::Ready { holder: C })
    );
}

#[test]
fn initial_untap_finishes_before_nonpositive_life_checks_at_upkeep() {
    let mut input = seed();
    input.life[0].total = 0.into();
    let candidate =
        GameState::initialize_scheduled(GameId(5), context(), input, TurnStart::default()).unwrap();
    assert_eq!(candidate.turn_transitions().len(), 2);
    assert_eq!(
        candidate.state().turn().unwrap().position().step,
        Step::Upkeep
    );
    assert_eq!(candidate.state().outcome(), Some(GameOutcome::Winner(B)));
}

#[test]
fn opening_rejects_even_zero_valued_seeded_mana_and_invalid_life() {
    for amount in [0, 1] {
        let mut input = opening_seed();
        input.mana.push(FloatingMana {
            player: A,
            mana_type: ManaType::Red,
            amount: amount.into(),
        });
        assert_eq!(
            GameState::initialize_opening(
                GameId(6),
                context(),
                OpeningSetup {
                    seed: input,
                    entropy: EntropyTape::new(vec![0; 100])
                }
            ),
            Err(GameError::Opening(OpeningError::InvalidSetup))
        );
    }
    let mut input = opening_seed();
    input.life.pop();
    assert_eq!(
        GameState::initialize_opening(
            GameId(6),
            context(),
            OpeningSetup {
                seed: input,
                entropy: EntropyTape::new(vec![0; 100])
            }
        ),
        Err(GameError::Effect(EffectError::InvalidSetup))
    );
}

#[test]
fn opening_retains_nonpositive_life_until_the_first_ordinary_check_boundary() {
    let mut input = opening_seed();
    input.life[0].total = 0.into();
    input.life[1].total = 40.into();
    let mut game = GameState::initialize_opening(
        GameId(7),
        context(),
        OpeningSetup {
            seed: input,
            entropy: EntropyTape::new(vec![0; 100]),
        },
    )
    .unwrap()
    .into_state();
    for _ in 0..2 {
        assert_eq!(game.execution().window().phase(), WindowPhase::Opening);
        assert!(game.execution().state_based_losses().is_empty());
        assert_eq!(game.outcome(), None);
        assert_eq!(game.execution().life_total(A), Some(&0.into()));
        game = keep(&game);
    }
    assert_eq!(game.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(game.outcome(), Some(GameOutcome::Winner(B)));
    assert_eq!(game.execution().life_total(B), Some(&40.into()));
}

#[path = "support/vocabulary.rs"]
mod synthetic_vocabulary;
fn test_vocabulary() -> std::sync::Arc<spellwake_contracts::creature_types::CreatureTypeVocabulary>
{
    synthetic_vocabulary::for_rules("CR20260925")
}
