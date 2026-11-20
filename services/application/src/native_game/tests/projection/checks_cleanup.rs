//! Public cleanup discard and concession originals across precommit departure.
use super::checks_order::{app, departure, project};
use super::chooser_replacement::{bound, four_seat_root_at};
use super::*;
use spellwake_engine::turn::Step;

const C: SeatId = SeatId(29);

#[test]
#[ignore = "requires genuine catalogue and vocabulary"]
fn cleanup_departure_preserves_private_hand_choice_or_finishes_the_ownerless_turn() {
    let source = four_seat_root_at(
        (1..=8)
            .map(|id| {
                card(
                    id,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(None, vec![]),
                )
            })
            .collect(),
        Step::Cleanup,
    );
    let viewer = app(&source, A);
    let mut original_scope = NativeScope::new(&source, bound(&source, viewer)).unwrap();
    let original_snapshot = project(&mut original_scope);
    let original_decision = &original_snapshot.snapshot().decisions[0];
    let PlayerDomain::Objects { objects, .. } = &original_decision.domain else {
        panic!("expected a private discard choice")
    };
    let original = command(
        &original_scope,
        PlayerAction::Choose {
            decision: original_decision.decision.clone(),
            response: PlayerChoice::Objects {
                objects: vec![objects[0].clone()],
            },
        },
    );
    let prepared = original_scope.propose(&original).unwrap();
    let stale = original_scope.encode_input(&prepared).unwrap();

    let active_left = departure(&source, A);
    assert!(active_left.check_journal().choices.is_empty());
    assert!(active_left.check_journal().moves.is_empty());
    let next = active_left.state();
    assert!(next.pending_decision().is_none());
    assert_eq!(next.execution().window().active_player(), Some(B));
    assert_eq!(next.turn().unwrap().position().number, 2);
    let mut survivor = NativeScope::new(next, bound(next, app(next, B))).unwrap();
    assert!(project(&mut survivor).snapshot().decisions.is_empty());

    let bystander = departure(&source, B);
    let next = bystander.state();
    assert!(bystander.check_journal().choices.is_empty());
    assert!(restore_input(next, bound(next, viewer), &stale).is_err());
    let mut scope = NativeScope::new(next, bound(next, viewer)).unwrap();
    let snapshot = project(&mut scope);
    let decision = &snapshot.snapshot().decisions[0];
    assert!(decision.kind == DecisionKind::Discard);
    assert!(matches!(
        crate::native_game::clocks::waiting(next, &scope.binding).unwrap(),
        crate::native_game::clocks::Waiting::Decision(seat) if seat == viewer
    ));
    let PlayerDomain::Objects { objects, .. } = &decision.domain else {
        panic!("expected the refreshed discard choice")
    };
    assert_eq!(objects.len(), 8);
    let reply = command(
        &scope,
        PlayerAction::Choose {
            decision: decision.decision.clone(),
            response: PlayerChoice::Objects {
                objects: vec![objects[0].clone()],
            },
        },
    );
    let mut observer = NativeScope::new(next, bound(next, app(next, C))).unwrap();
    assert!(project(&mut observer).snapshot().decisions.is_empty());
    assert!(observer.reference(&objects[0]).is_err());
    assert!(observer.propose(&reply).is_err());
    let prepared = scope.propose(&reply).unwrap();
    input_codec::verify(&scope, &prepared);
    assert_eq!(prepared.candidate().check_journal().choices.len(), 1);
    assert_eq!(prepared.candidate().check_journal().moves.len(), 1);
    let finished = prepared.candidate().state();
    assert_eq!(finished.execution().window().active_player(), Some(C));
    assert!(finished.pending_decision().is_none());
    let mut scope = NativeScope::new(finished, bound(finished, viewer)).unwrap();
    project(&mut scope);
}
