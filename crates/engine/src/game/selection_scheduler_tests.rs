//! Synthetic native composition: actual cleanup priority and repeated cleanup.
//! No source-card, full-format or ordinary observer qualification is implied.
use super::*;
use std::collections::BTreeSet;

#[path = "selection_scheduler_tests/expiry_and_tokens.rs"]
mod expiry_and_tokens;

struct SavedCall {
    input: GameInput,
    result: Result<NativeCandidate, GameError>,
}

fn recorded(
    state: &mut GameState,
    actor: SeatId,
    command: GameCommand,
    rows: &mut Vec<RulesRecord>,
    calls: &mut Vec<SavedCall>,
) -> NativeCandidate {
    let before = state.clone();
    let input = GameInput {
        basis: before.basis().clone(),
        actor,
        command: command.clone(),
    };
    let actual = accept(state, actor, command, rows);
    assert_eq!(before.advance(input.clone()), Ok(actual.clone()));
    calls.push(SavedCall {
        input,
        result: Ok(actual.clone()),
    });
    actual
}

fn recorded_reply(
    state: &mut GameState,
    choice: DecisionReply,
    rows: &mut Vec<RulesRecord>,
    calls: &mut Vec<SavedCall>,
) -> NativeCandidate {
    let decision = state.pending_decision().unwrap();
    recorded(
        state,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
        rows,
        calls,
    )
}

fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn replay_calls(initial: GameState, expected: &GameState, calls: Vec<SavedCall>) {
    let mut replayed = initial;
    for call in calls {
        let before = replayed.clone();
        let actual = replayed.advance(call.input);
        assert_eq!(replayed, before);
        assert_eq!(actual, call.result, "complete original typed-input replay");
        if let Ok(candidate) = actual {
            replayed = candidate.into_state();
        }
    }
    assert_eq!(&replayed, expected);
}

#[test]
fn actual_cleanup_discard_does_not_enter_effect_only_replacement_work() {
    use crate::program::{
        PublicZoneDestination, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    };
    let mut cards = libraries(&[A, B]);
    let mut replacement = Program::enchantment(None);
    replacement.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(1),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    cards.push(SeedCard {
        card: CardId(90),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: replacement,
    });
    for card in 10..18 {
        cards.push(SeedCard {
            card: CardId(card),
            owner: A,
            destination: ObjectDestination::Hand,
            program: land(ManaType::White),
        });
    }
    let initialized = scheduled(vec![A, B], A, Step::End, cards, vec![]);
    let initial = initialized.state().clone();
    let mut state = initial.clone();
    let mut rows = initialized.rules_journal().records.clone();
    let mut calls = vec![];
    for actor in [A, B] {
        recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
    }
    let RuleFrame::Checks(crate::checks::CheckDecision::CleanupDiscard(frame)) =
        state.pending_decision().unwrap().frame()
    else {
        panic!("actual cleanup discard")
    };
    assert_eq!(frame.required(), 1);
    let selected = object(&state, 10);
    let result = recorded_reply(
        &mut state,
        DecisionReply::Discard(vec![selected]),
        &mut rows,
        &mut calls,
    );
    let moved = state
        .execution()
        .window()
        .cards()
        .get(object(&state, 10))
        .unwrap();
    assert_eq!(
        moved.zone(),
        crate::object_identity::ObjectZone::Graveyard(A)
    );
    assert!(!matches!(
        state.pending_decision().map(|d| d.frame()),
        Some(RuleFrame::ZoneReplacement(_))
    ));
    assert!(result.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|row|matches!(&row.value,crate::occurrence::RulesOccurrence::ZoneChange(change) if change.before.object.id()==selected && change.reason==crate::occurrence::ZoneReason::Discard {player:A})))));
    replay_calls(initial, &state, calls);
}
