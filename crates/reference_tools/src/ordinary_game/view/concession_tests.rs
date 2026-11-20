//! Actual historical root admission/bytes, retained before concession changes.
use super::*;
use spellwake_engine::{
    checks::CheckDecision,
    object_identity::ObjectDestination,
    setup::GameSeed,
    turn::{Step, TurnStart},
};

fn checked_cleanup() -> (Fixture, History, GameState, GameInput, NativeCandidate) {
    let fixture = Fixture::new(
        super::super::fixture::CaseId::G01,
        crate::creature_types::selected_vocabulary().unwrap(),
    )
    .unwrap();
    let mut seed: GameSeed = fixture.setup.seed.clone();
    let mut hand = 0;
    for card in &mut seed.cards {
        card.destination = if card.owner == A && hand < 8 {
            hand += 1;
            ObjectDestination::Hand
        } else {
            ObjectDestination::LibraryBottom
        };
    }
    seed.initial_battlefield_order.clear();
    seed.mana.clear();
    seed.life
        .iter_mut()
        .find(|row| row.player == A)
        .unwrap()
        .total = 0.into();
    let initial = GameState::initialize_scheduled(
        fixture.game,
        fixture.context.clone(),
        seed,
        TurnStart {
            number: u64::MAX,
            step: Step::Cleanup,
        },
    )
    .unwrap();
    let mut history = History::default();
    history.remember(None, &initial);
    let source = initial.into_state();
    assert!(source.outcome().is_none());
    assert!(source.execution().state_based_losses().is_empty());
    let decision = source.pending_decision().unwrap();
    let RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) = decision.frame() else {
        panic!("real scheduled cleanup discard");
    };
    let input = GameInput {
        basis: source.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::Discard(vec![frame.domain()[0]]),
        },
    };
    let accepted = source.advance(input.clone()).unwrap();
    assert_eq!(source.advance(input.clone()), Ok(accepted.clone()));
    assert_eq!(accepted.state().outcome(), Some(GameOutcome::Winner(B)));
    assert!(accepted.state().pending_decision().is_none());
    assert_eq!(accepted.state().turn(), source.turn());
    history.remember(Some(&source), &accepted);
    (fixture, history, source, input, accepted)
}

#[test]
#[ignore = "genuine vocabulary and explicit fresh output required; historical byte capture"]
fn concession_checked_cleanup_historical_capture() {
    let (fixture, history, source, issued, accepted) = checked_cleanup();
    assert!(accepted.state().pending_turn_work().is_none());
    assert!(
        matches!(accepted.state().conclusion().unwrap().cause(),spellwake_engine::game::GameConclusionCause::CheckedLosses {players} if players==&std::collections::BTreeSet::from([A]))
    );
    source.check_current10().unwrap();
    accepted.state().check_current10().unwrap();
    accepted.check_current10().unwrap();
    let observed = serde_json::json!({
        "source_header":header(&fixture,&history,&source).unwrap(),
        "input":input(&fixture,&history,&issued).unwrap(),
        "header":header(&fixture,&history,accepted.state()).unwrap(),
        "world":world(&fixture,&history,accepted.state()).unwrap(),
        "facts":facts(&fixture,&history,Some(&source),&accepted).unwrap(),
        "rules_journal":projection::journal(accepted.rules_journal()),
        "source_current10":true,"state_current10":true,"candidate_current10":true,
    });
    assert_eq!(observed["header"]["pending_turn_work"]["kind"], "cleanup");
    let bytes = serde_json::to_vec(&observed).unwrap();
    if let Some(path) = std::env::var_os("SPELLWAKE_CONCESSION_CHECKED_BASELINE") {
        assert_eq!(
            bytes,
            std::fs::read(path).unwrap(),
            "actual historical cleanup bytes"
        );
    }
    let path = std::env::var_os("SPELLWAKE_CONCESSION_CHECKED_OUTPUT").unwrap();
    use std::io::Write;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap()
        .write_all(&bytes)
        .unwrap();
}

#[test]
fn concession_ordinary_root_input_facts_and_isolated_journal_refuse() {
    let (fixture, history, source, _, _) = checked_cleanup();
    let issued = GameInput {
        basis: source.basis().clone(),
        actor: B,
        command: GameCommand::Concede,
    };
    let accepted = source.advance(issued.clone()).unwrap();
    assert!(input(&fixture, &history, &issued).is_err());
    let mut rejected = issued;
    rejected.actor = spellwake_engine::priority::SeatId(999);
    assert!(source.advance(rejected.clone()).is_err());
    assert!(input(&fixture, &history, &rejected).is_err());
    assert!(header(&fixture, &history, accepted.state()).is_err());
    assert!(world(&fixture, &history, accepted.state()).is_err());
    assert!(facts(&fixture, &history, Some(&source), &accepted).is_err());
    assert!(check_predecessor_rules_journal(accepted.rules_journal()).is_err());
    let checked = checked_cleanup().4;
    header(&fixture, &history, checked.state()).unwrap();
    facts(&fixture, &history, Some(&source), &checked).unwrap();
}
