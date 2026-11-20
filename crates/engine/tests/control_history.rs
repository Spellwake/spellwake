//! Public primitive behavior for incarnation-bound continuous-control history.
//! These trusted registry transitions do not execute control effects or combat.
use spellwake_engine::{
    object_identity::{CardId, GameObjects, ObjectDestination, ObjectId, ObjectIdentityError},
    priority::SeatId,
};

const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);

fn seeded(destination: ObjectDestination) -> (GameObjects, ObjectId) {
    let change = GameObjects::new(0)
        .seed_card(0, CardId(17), A, destination)
        .unwrap();
    (change.next, change.after.id())
}

fn history(cards: &GameObjects, object: ObjectId) -> Option<bool> {
    cards
        .get(object)
        .unwrap()
        .has_continuous_control_since_turn_start()
}

#[test]
fn only_battlefield_incarnations_have_control_history_and_fresh_entry_is_unstamped() {
    for destination in [
        ObjectDestination::LibraryTop,
        ObjectDestination::Hand,
        ObjectDestination::Graveyard,
        ObjectDestination::Battlefield { controller: A },
        ObjectDestination::Stack { controller: A },
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ] {
        let (cards, object) = seeded(destination);
        let battlefield = matches!(destination, ObjectDestination::Battlefield { .. });
        assert_eq!(
            cards.get(object).unwrap().control_epoch().is_some(),
            battlefield
        );
        assert_eq!(history(&cards, object), battlefield.then_some(false));
    }
}

#[test]
fn turn_start_stamps_all_controlled_permanents_without_changing_physical_state() {
    let mut cards = GameObjects::new(0);
    for (id, owner, destination) in [
        (1, A, ObjectDestination::Battlefield { controller: A }),
        (2, B, ObjectDestination::Battlefield { controller: A }),
        (3, A, ObjectDestination::Battlefield { controller: B }),
        (4, B, ObjectDestination::Battlefield { controller: B }),
        (5, A, ObjectDestination::Hand),
        (6, A, ObjectDestination::Stack { controller: A }),
        (7, A, ObjectDestination::LibraryBottom),
        (8, A, ObjectDestination::LibraryBottom),
    ] {
        cards = cards
            .seed_card(cards.revision(), CardId(id), owner, destination)
            .unwrap()
            .next;
    }
    let first = cards.current_identity(CardId(1)).unwrap();
    cards = cards
        .set_tapped(cards.revision(), first, true)
        .unwrap()
        .next;
    let saved = cards.clone();
    let marked = cards.record_turn_start(cards.revision(), A).unwrap();
    assert_eq!(cards, saved);
    assert_eq!(marked.revision(), cards.revision() + 1);
    assert_eq!(marked.iter().count(), cards.iter().count());
    assert_eq!(marked.library(A), cards.library(A));
    for before in cards.iter() {
        let after = marked.get(before.id()).unwrap();
        assert_eq!(
            after.card_id().expect("physical fixture card"),
            before.card_id().expect("physical fixture card")
        );
        assert_eq!(after.owner(), before.owner());
        assert_eq!(after.zone(), before.zone());
        assert_eq!(after.controller(), before.controller());
        assert_eq!(after.tapped(), before.tapped());
        assert_eq!(after.control_epoch(), before.control_epoch());
        let expected = match before.card_id().expect("physical fixture card") {
            CardId(1 | 2) => Some(true),
            CardId(3 | 4) => Some(false),
            _ => None,
        };
        assert_eq!(after.has_continuous_control_since_turn_start(), expected);
    }
}

#[test]
fn reaffirming_controller_preserves_both_fresh_and_established_history() {
    let (cards, object) = seeded(ObjectDestination::Battlefield { controller: A });
    let epoch = cards.get(object).unwrap().control_epoch();
    let same = cards.apply_controller(cards.revision(), object, A).unwrap();
    assert_eq!(same.after.control_epoch(), epoch);
    assert_eq!(
        same.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
    assert_eq!(same.next.revision(), cards.revision() + 1);
    let marked = same
        .next
        .record_turn_start(same.next.revision(), A)
        .unwrap();
    let saved = marked.clone();
    let same = marked
        .apply_controller(marked.revision(), object, A)
        .unwrap();
    assert_eq!(marked, saved);
    assert_eq!(same.after.id(), object);
    assert_eq!(same.after.control_epoch(), epoch);
    assert_eq!(
        same.after.has_continuous_control_since_turn_start(),
        Some(true)
    );
}

#[test]
fn away_and_back_control_cannot_reuse_an_earlier_turn_start() {
    let (cards, object) = seeded(ObjectDestination::Battlefield { controller: A });
    let tapped = cards
        .set_tapped(cards.revision(), object, true)
        .unwrap()
        .next;
    let marked = tapped.record_turn_start(tapped.revision(), A).unwrap();
    let original_epoch = marked.get(object).unwrap().control_epoch();
    let saved = marked.clone();
    let away = marked
        .apply_controller(marked.revision(), object, B)
        .unwrap();
    assert_eq!(marked, saved);
    assert_eq!(away.after.id(), object);
    assert_eq!(away.after.owner(), A);
    assert_eq!(away.after.controller(), Some(B));
    assert_eq!(away.after.tapped(), Some(true));
    assert_ne!(away.after.control_epoch(), original_epoch);
    assert_eq!(
        away.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let away_epoch = away.after.control_epoch();
    let back = away
        .next
        .apply_controller(away.next.revision(), object, A)
        .unwrap();
    assert_ne!(back.after.control_epoch(), original_epoch);
    assert_ne!(back.after.control_epoch(), away_epoch);
    assert_eq!(back.after.id(), object);
    assert_eq!(back.after.tapped(), Some(true));
    assert_eq!(
        back.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let other_turn = back
        .next
        .record_turn_start(back.next.revision(), B)
        .unwrap();
    assert_eq!(history(&other_turn, object), Some(false));
    let next_own_turn = other_turn
        .record_turn_start(other_turn.revision(), A)
        .unwrap();
    assert_eq!(history(&next_own_turn, object), Some(true));
    assert_eq!(
        next_own_turn.get(object).unwrap().control_epoch(),
        back.after.control_epoch()
    );
}

#[test]
fn the_new_controllers_turn_establishes_history_without_changing_the_epoch() {
    let (cards, object) = seeded(ObjectDestination::Battlefield { controller: A });
    let marked = cards.record_turn_start(cards.revision(), A).unwrap();
    let changed = marked
        .apply_controller(marked.revision(), object, B)
        .unwrap();
    let epoch = changed.after.control_epoch();
    let old_controller_turn = changed
        .next
        .record_turn_start(changed.next.revision(), A)
        .unwrap();
    assert_eq!(history(&old_controller_turn, object), Some(false));
    let own_turn = old_controller_turn
        .record_turn_start(old_controller_turn.revision(), B)
        .unwrap();
    assert_eq!(history(&own_turn, object), Some(true));
    assert_eq!(own_turn.get(object).unwrap().control_epoch(), epoch);
}

#[test]
fn tapping_and_untapping_preserve_established_history_and_current_incarnation() {
    let (cards, object) = seeded(ObjectDestination::Battlefield { controller: A });
    let marked = cards.record_turn_start(cards.revision(), A).unwrap();
    let epoch = marked.get(object).unwrap().control_epoch();
    let tapped = marked.set_tapped(marked.revision(), object, true).unwrap();
    let untapped = tapped
        .next
        .set_tapped(tapped.next.revision(), object, false)
        .unwrap();
    for card in [&tapped.after, &untapped.after] {
        assert_eq!(card.id(), object);
        assert_eq!(card.control_epoch(), epoch);
        assert_eq!(card.has_continuous_control_since_turn_start(), Some(true));
    }
}

#[test]
fn a_card_entering_after_the_turn_start_waits_for_its_controllers_next_start() {
    let (cards, old_object) = seeded(ObjectDestination::Battlefield { controller: A });
    let marked = cards.record_turn_start(cards.revision(), A).unwrap();
    let later = marked
        .seed_card(
            marked.revision(),
            CardId(18),
            A,
            ObjectDestination::Battlefield { controller: A },
        )
        .unwrap();
    let new_object = later.after.id();
    assert_eq!(history(&later.next, old_object), Some(true));
    assert_eq!(history(&later.next, new_object), Some(false));
    let other_turn = later
        .next
        .record_turn_start(later.next.revision(), B)
        .unwrap();
    assert_eq!(history(&other_turn, new_object), Some(false));
    let own_turn = other_turn
        .record_turn_start(other_turn.revision(), A)
        .unwrap();
    assert_eq!(history(&own_turn, old_object), Some(true));
    assert_eq!(history(&own_turn, new_object), Some(true));
}

#[test]
fn departure_removes_history_and_return_cannot_follow_the_stable_card_key() {
    for destination in [
        ObjectDestination::LibraryBottom,
        ObjectDestination::Hand,
        ObjectDestination::Graveyard,
        ObjectDestination::Stack { controller: B },
        ObjectDestination::Exile,
        ObjectDestination::Command,
    ] {
        let (cards, old_object) = seeded(ObjectDestination::Battlefield { controller: A });
        let marked = cards.record_turn_start(cards.revision(), A).unwrap();
        let epoch = marked.get(old_object).unwrap().control_epoch();
        let departed = marked
            .apply_zone_move(marked.revision(), old_object, destination)
            .unwrap()
            .require_moved()
            .unwrap();
        assert_eq!(departed.after.control_epoch(), None);
        assert_eq!(
            departed.after.has_continuous_control_since_turn_start(),
            None
        );
        assert!(departed.next.get(old_object).is_none());
        let returned = departed
            .next
            .apply_zone_move(
                departed.next.revision(),
                departed.after.id(),
                ObjectDestination::Battlefield { controller: A },
            )
            .unwrap()
            .require_moved()
            .unwrap();
        assert_ne!(returned.after.id(), old_object);
        assert_eq!(
            returned.after.card_id().expect("physical fixture card"),
            CardId(17)
        );
        assert_ne!(returned.after.control_epoch(), epoch);
        assert_eq!(
            returned.after.has_continuous_control_since_turn_start(),
            Some(false)
        );
        assert_eq!(
            returned
                .next
                .apply_controller(returned.next.revision(), old_object, B),
            Err(ObjectIdentityError::UnknownObject)
        );
    }
}

#[test]
fn stack_control_changes_never_acquire_permanent_history() {
    let (cards, object) = seeded(ObjectDestination::Stack { controller: A });
    let changed = cards.apply_controller(cards.revision(), object, B).unwrap();
    assert_eq!(changed.after.id(), object);
    assert_eq!(changed.after.control_epoch(), None);
    assert_eq!(
        changed.after.has_continuous_control_since_turn_start(),
        None
    );
    let stamped = changed
        .next
        .record_turn_start(changed.next.revision(), B)
        .unwrap();
    assert_eq!(history(&stamped, object), None);
    let permanent = stamped
        .apply_zone_move(
            stamped.revision(),
            object,
            ObjectDestination::Battlefield { controller: B },
        )
        .unwrap()
        .require_moved()
        .unwrap();
    assert_eq!(
        permanent.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
}
