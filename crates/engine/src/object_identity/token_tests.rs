//! Shared-identity invariants under the retained source-qualified vocabulary.
use super::*;
use crate::{characteristics::Color, test_support::creature_types::selected_vocabulary};
use spellwake_contracts::names::{EnglishCardName, ObjectNames};

fn definition() -> Program {
    let mut program = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    program.names = ObjectNames::single(EnglishCardName::new("Token".into()).unwrap());
    program.effect_defined_colors = Some(BTreeSet::from([Color::Green]));
    program
}
fn created() -> ObjectChange {
    let vocabulary = selected_vocabulary().unwrap();
    GameObjects::new(0)
        .insert_token(0, SeatId(0), definition(), &vocabulary)
        .unwrap()
}
#[test]
fn tokens_share_the_incarnation_allocator_without_a_physical_index_or_card_pile() {
    let v = selected_vocabulary().unwrap();
    let card = GameObjects::new(0)
        .seed_card(0, CardId(u64::MAX), SeatId(1), ObjectDestination::Graveyard)
        .unwrap();
    let token = card
        .next
        .insert_token(card.next.revision(), SeatId(0), definition(), &v)
        .unwrap();
    assert_ne!(token.after.id(), card.after.id());
    assert_eq!(
        token.next.next_identity_sequence(),
        card.next.next_identity_sequence() + 1
    );
    assert_eq!(token.after.card_id(), None);
    assert!(token.after.is_token());
    assert_eq!(token.after.owner(), SeatId(0));
    assert_eq!(token.after.controller(), Some(SeatId(0)));
    assert_eq!(token.after.tapped(), Some(false));
    assert_eq!(
        token.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(
        token.next.current_identity(CardId(u64::MAX)),
        Some(card.after.id())
    );
    assert_eq!(token.next.graveyard(SeatId(1)), &[card.after.id()]);
    assert!(token.next.graveyard(SeatId(0)).is_empty());
    assert_eq!(
        token.after.base_program(&BTreeMap::new()),
        Some(&definition())
    );
    let encoded = crate::test_support::damage_trace::game_object_view(&card.after);
    assert_eq!(
        serde_json::to_value(encoded).unwrap()["origin"]["card_id"].as_u64(),
        Some(u64::MAX)
    );
    let encoded = serde_json::to_value(crate::test_support::damage_trace::game_object_view(
        &token.after,
    ))
    .unwrap();
    assert!(encoded["physical_card"].is_null());
    assert_eq!(encoded["origin"]["kind"], "token");
}
#[test]
fn first_departure_preserves_origin_and_owner_then_all_later_moves_are_unchanged() {
    let token = created();
    let controlled = token
        .next
        .apply_controller(token.next.revision(), token.after.id(), SeatId(1))
        .unwrap();
    let moved = controlled
        .next
        .apply_zone_move(
            controlled.next.revision(),
            token.after.id(),
            ObjectDestination::Hand,
        )
        .unwrap()
        .require_moved()
        .unwrap();
    assert_ne!(moved.after.id(), token.after.id());
    assert_eq!(moved.after.origin(), token.after.origin());
    assert_eq!(moved.after.zone(), ObjectZone::Hand(SeatId(0)));
    assert_eq!(moved.after.controller(), None);
    assert_eq!(moved.after.tapped(), None);
    assert_eq!(moved.after.control_epoch(), None);
    for destination in [
        ObjectDestination::Hand,
        ObjectDestination::LibraryTop,
        ObjectDestination::LibraryBottom,
        ObjectDestination::Graveyard,
        ObjectDestination::Battlefield {
            controller: SeatId(1),
        },
        ObjectDestination::Stack {
            controller: SeatId(1),
        },
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ] {
        let result = moved
            .next
            .apply_zone_move(moved.next.revision(), moved.after.id(), destination)
            .unwrap();
        assert_eq!(result.next, moved.next);
        assert_eq!(
            result.outcome,
            ZoneMoveOutcome::Unchanged {
                object: moved.after.clone(),
                reason: UnchangedMoveReason::DepartedToken
            }
        );
        assert_eq!(
            result.require_moved(),
            Err(ObjectIdentityError::UnexpectedUnchangedMove)
        );
    }
    let mut exhausted = moved.next.clone();
    exhausted.set_test_revision(u64::MAX);
    exhausted.set_test_next_identity(u64::MAX);
    assert_eq!(
        exhausted
            .apply_zone_move(u64::MAX, moved.after.id(), ObjectDestination::Exile)
            .unwrap()
            .next,
        exhausted
    );
    assert_eq!(
        exhausted.apply_zone_move(0, moved.after.id(), ObjectDestination::Exile),
        Err(ObjectIdentityError::StaleRevision)
    );
    let ceased = moved
        .next
        .cease_token(moved.next.revision(), moved.after.id())
        .unwrap();
    assert_eq!(ceased.object, moved.after);
    assert_eq!(
        ceased.next.next_identity_sequence(),
        moved.next.next_identity_sequence()
    );
    assert!(ceased.next.get(ceased.object.id()).is_none());
    assert!(ceased.next.iter().next().is_none());
    assert_eq!(
        ceased.object.base_program(&BTreeMap::new()),
        Some(&definition())
    );
}
#[test]
fn mixed_move_batch_retains_unchanged_rows_and_rejects_a_late_fault_atomically() {
    let token = created();
    let token = token
        .next
        .apply_zone_move(
            token.next.revision(),
            token.after.id(),
            ObjectDestination::Exile,
        )
        .unwrap()
        .require_moved()
        .unwrap();
    let card = token
        .next
        .seed_card(
            token.next.revision(),
            CardId(9),
            SeatId(1),
            ObjectDestination::Hand,
        )
        .unwrap();
    let moves = [
        (token.after.id(), ObjectDestination::Graveyard),
        (card.after.id(), ObjectDestination::Graveyard),
    ];
    let (next, outcomes) = card
        .next
        .apply_zone_moves(card.next.revision(), &moves)
        .unwrap();
    assert_eq!(outcomes.len(), 2);
    assert!(matches!(outcomes[0], ZoneMoveOutcome::Unchanged { .. }));
    assert!(matches!(outcomes[1], ZoneMoveOutcome::Moved { .. }));
    assert_eq!(next.revision(), card.next.revision() + 1);
    assert_eq!(
        next.next_identity_sequence(),
        card.next.next_identity_sequence() + 1
    );
    assert_eq!(next.graveyard(SeatId(1)).len(), 1);
    assert!(next.graveyard(SeatId(0)).is_empty());
    let before = card.next.clone();
    let mut invalid = moves.to_vec();
    invalid.push((ObjectId(u64::MAX), ObjectDestination::Exile));
    assert_eq!(
        card.next.apply_zone_moves(card.next.revision(), &invalid),
        Err(ObjectIdentityError::UnknownObject)
    );
    assert_eq!(card.next, before);
}
#[test]
fn insertion_and_cessation_failures_preserve_all_registry_state() {
    let v = selected_vocabulary().unwrap();
    let token = created();
    let mut bad = definition();
    bad.names = ObjectNames::nameless();
    assert!(
        token
            .next
            .insert_token(token.next.revision(), SeatId(0), bad, &v)
            .is_err()
    );
    assert_eq!(
        token
            .next
            .cease_token(token.next.revision(), token.after.id()),
        Err(ObjectIdentityError::NotDepartedToken)
    );
    for identity_exhausted in [false, true] {
        let mut state = token.next.clone();
        let expected = if identity_exhausted {
            state.set_test_next_identity(u64::MAX);
            ObjectIdentityError::IdentityExhausted
        } else {
            state.set_test_revision(u64::MAX);
            ObjectIdentityError::RevisionExhausted
        };
        let before = state.clone();
        assert_eq!(
            state.insert_token(state.revision(), SeatId(0), definition(), &v),
            Err(expected)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn card_only_cast_and_land_primitives_reject_tokens_before_allocating() {
    use crate::stack_window::{StackWindow, StackWindowError};
    let token = created();
    let window =
        StackWindow::new(vec![SeatId(0), SeatId(1)], SeatId(0), token.next.clone()).unwrap();
    let window = window.finish_checks(window.revision()).unwrap();
    let before = window.clone();
    assert_eq!(
        window.record_card_spell(window.revision(), SeatId(0), token.after.id()),
        Err(StackWindowError::Objects(ObjectIdentityError::NotCard))
    );
    assert_eq!(window, before);
    // Explicit lower prepared transient state, not a live game/checks trajectory.
    let departed = token
        .next
        .apply_zone_move(
            token.next.revision(),
            token.after.id(),
            ObjectDestination::Hand,
        )
        .unwrap()
        .require_moved()
        .unwrap();
    let window = StackWindow::new(vec![SeatId(0), SeatId(1)], SeatId(0), departed.next).unwrap();
    let window = window.finish_checks(window.revision()).unwrap();
    let before = window.clone();
    assert_eq!(
        window.record_land_play(window.revision(), SeatId(0), departed.after.id()),
        Err(StackWindowError::Objects(ObjectIdentityError::NotCard))
    );
    assert_eq!(window, before);
}
#[test]
fn token_control_history_and_tapping_use_the_same_exact_incarnation() {
    let token = created();
    let id = token.after.id();
    let started = token
        .next
        .record_turn_start(token.next.revision(), SeatId(0))
        .unwrap();
    assert_eq!(
        started
            .get(id)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(true)
    );
    let tapped = started.set_tapped(started.revision(), id, true).unwrap();
    assert_eq!(tapped.after.origin(), token.after.origin());
    assert_eq!(tapped.after.id(), id);
    let changed = tapped
        .next
        .apply_controller(tapped.next.revision(), id, SeatId(1))
        .unwrap();
    assert_eq!(changed.after.owner(), SeatId(0));
    assert_eq!(changed.after.card_id(), None);
    assert_eq!(changed.after.tapped(), Some(true));
    assert_eq!(
        changed.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let started = changed
        .next
        .record_turn_start(changed.next.revision(), SeatId(1))
        .unwrap();
    assert_eq!(
        started
            .get(id)
            .unwrap()
            .has_continuous_control_since_turn_start(),
        Some(true)
    );
    assert_eq!(started.get(id).unwrap().origin(), token.after.origin());
}
