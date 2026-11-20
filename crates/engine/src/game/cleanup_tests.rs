//! Root atomicity tests using explicitly prepared ordinary cleanup worlds.
use super::*;
use crate::{
    effects::{Program, SeedCard},
    object_identity::{CardId, ObjectDestination},
};
const A: SeatId = SeatId(41);
const B: SeatId = SeatId(7);
fn prepared(number: u64, zero_creatures: bool, life: i64) -> GameState {
    let mut cards: Vec<_> = (1..=8)
        .map(|id| SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::creature(None, 2.into(), 2.into(), std::collections::BTreeSet::new()),
        })
        .collect();
    if zero_creatures {
        cards.extend((20..=21).map(|id| SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: B },
            program: Program::creature(None, 3.into(), 0.into(), std::collections::BTreeSet::new()),
        }));
    }
    let mut seed = {
        // Fixture premise: seeded BF timestamps follow this authored card sequence.
        let initial_battlefield_order = cards
            .iter()
            .filter(|card| {
                matches!(
                    card.destination,
                    crate::object_identity::ObjectDestination::Battlefield { .. }
                )
            })
            .map(|card| card.card)
            .collect();
        GameSeed::with_uniform_life(
            vec![A, B],
            A,
            cards,
            initial_battlefield_order,
            vec![FloatingMana {
                player: B,
                mana_type: crate::mana::ManaType::Red,
                amount: 5.into(),
            }],
            20.into(),
            CombatRules::UnrestrictedFreeForAll,
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("rules"),
            std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
        )
    };
    seed.life[0].total = life.into();
    GameState::initialize_scheduled(
        GameId(808),
        GameContext::new(
            "release".into(),
            "rules".into(),
            "cards".into(),
            "programs".into(),
            "cleanup-fault".into(),
            crate::test_support::creature_types::synthetic_vocabulary_for_rules("rules").id(),
            spellwake_contracts::names::NameRules::literal_names().id(),
        )
        .unwrap(),
        seed,
        TurnStart {
            number,
            step: Step::Cleanup,
        },
    )
    .unwrap()
    .into_state()
}
fn discard(game: &GameState) -> GameInput {
    let pending = game.pending_decision().unwrap();
    let RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) = pending.frame() else {
        panic!("actual cleanup discard required");
    };
    GameInput {
        basis: game.basis.clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::Discard(vec![frame.domain()[0]]),
        },
    }
}
#[test]
fn late_owner_decision_exhaustion_rolls_back_discard_and_new_check_work() {
    let mut game = prepared(3, true, 20);
    let valid = game.advance(discard(&game)).unwrap();
    assert_eq!(valid.check_journal().moves.len(), 1);
    assert!(matches!(
        valid.state().pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::GraveyardOrder(_))
    ));
    game.next_decision = u64::MAX;
    let before = game.clone();
    assert_eq!(
        game.advance(discard(&game)),
        Err(GameError::DecisionIdentityExhausted)
    );
    assert_eq!(game, before);
    assert_eq!(game.execution().window().cards().graveyard(A), &[]);
    assert_eq!(
        game.execution().mana(B, crate::mana::ManaType::Red),
        5.into()
    );
}
#[test]
fn late_next_turn_exhaustion_returns_no_discard_candidate() {
    let game = prepared(u64::MAX, false, 20);
    let before = game.clone();
    assert!(matches!(
        game.advance(discard(&game)),
        Err(GameError::Turn(TurnError::TurnExhausted))
    ));
    assert_eq!(game, before);
    assert_eq!(game.execution().window().cards().graveyard(A), &[]);
    assert!(game.pending_decision().is_some());
}
#[test]
fn committed_cleanup_loss_does_not_try_to_advance_an_exhausted_turn() {
    let game = prepared(u64::MAX, false, 0);
    assert!(game.outcome().is_none());
    assert!(game.execution().state_based_losses().is_empty());
    let accepted = game.advance(discard(&game)).unwrap();
    assert_eq!(accepted.state().outcome(), Some(GameOutcome::Winner(B)));
    assert!(accepted.state().pending_decision().is_none());
    assert_eq!(
        accepted.state().turn().unwrap().position(),
        game.turn().unwrap().position()
    );
    assert_eq!(accepted.check_journal().moves.len(), 1);
    assert_eq!(accepted.check_journal().moves[0].event.pass(), 0);
    assert!(accepted.turn_transitions().is_empty());
}
#[test]
fn exhausted_root_basis_preserves_the_actual_pending_discard() {
    let mut game = prepared(3, false, 20);
    game.basis.revision = u64::MAX;
    let before = game.clone();
    assert_eq!(
        game.advance(discard(&game)),
        Err(GameError::RevisionExhausted)
    );
    assert_eq!(game, before);
}
