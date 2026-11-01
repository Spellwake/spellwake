//! Independent physical-status acceptance from the preimplementation
//! mana-abilities/reviews/native-scenarios.md matrix, N25–N30.
use spellwake_engine::{
    card_identity::{
        CardDestination, CardId, CardIdentityError, CardObjectId, CardObjects, CardZone,
    },
    priority::SeatId,
    stack_window::{StackWindow, StackWindowError},
};

const OWNER: SeatId = SeatId(0);
const OTHER: SeatId = SeatId(1);

fn destinations() -> [CardDestination; 7] {
    [
        CardDestination::Library,
        CardDestination::Hand,
        CardDestination::Graveyard,
        CardDestination::Battlefield { controller: OWNER },
        CardDestination::Stack { controller: OWNER },
        CardDestination::Exile,
        CardDestination::Command,
    ]
}

fn seeded(destination: CardDestination) -> (CardObjects, CardObjectId) {
    let change = CardObjects::new(0)
        .seed_card(0, CardId(9), OWNER, destination)
        .unwrap();
    (change.next, change.after.id())
}

#[test]
fn status_exists_exactly_on_battlefield_including_stack_with_a_controller() {
    for destination in destinations() {
        let (cards, id) = seeded(destination);
        let card = cards.get(id).unwrap();
        let expected = match destination {
            CardDestination::Battlefield { .. } => Some(false),
            _ => None,
        };
        assert_eq!(card.tapped(), expected);
        if matches!(destination, CardDestination::Stack { .. }) {
            assert_eq!(card.controller(), Some(OWNER));
            assert_eq!(card.tapped(), None);
        }
    }
}

#[test]
fn tap_and_untap_change_status_and_revision_without_replacing_the_object() {
    let (cards, id) = seeded(CardDestination::Battlefield { controller: OWNER });
    let untapped = cards.get(id).unwrap().clone();
    let saved = cards.clone();
    let tapped = cards.set_tapped(cards.revision(), id, true).unwrap();
    assert_eq!(cards, saved);
    assert_eq!(tapped.before, Some(untapped));
    assert_eq!(tapped.after.id(), id);
    assert_eq!(tapped.after.card(), CardId(9));
    assert_eq!(tapped.after.owner(), OWNER);
    assert_eq!(tapped.after.zone(), CardZone::Battlefield);
    assert_eq!(tapped.after.controller(), Some(OWNER));
    assert_eq!(tapped.after.tapped(), Some(true));
    assert_eq!(tapped.next.revision(), cards.revision() + 1);
    assert_eq!(tapped.next.current_identity(CardId(9)), Some(id));
    assert_eq!(tapped.next.iter().count(), 1);

    let saved = tapped.next.clone();
    let untapped = tapped
        .next
        .set_tapped(tapped.next.revision(), id, false)
        .unwrap();
    assert_eq!(tapped.next, saved);
    assert_eq!(untapped.before, Some(tapped.after));
    assert_eq!(untapped.after.id(), id);
    assert_eq!(untapped.after.tapped(), Some(false));
    assert_eq!(untapped.next.revision(), saved.revision() + 1);
}

#[test]
fn changing_control_preserves_tapped_status_and_same_incarnation() {
    let (cards, id) = seeded(CardDestination::Battlefield { controller: OWNER });
    let tapped = cards.set_tapped(cards.revision(), id, true).unwrap();
    let stolen = tapped
        .next
        .apply_controller(tapped.next.revision(), id, OTHER)
        .unwrap();
    assert_eq!(stolen.before, Some(tapped.after));
    assert_eq!(stolen.after.id(), id);
    assert_eq!(stolen.after.owner(), OWNER);
    assert_eq!(stolen.after.controller(), Some(OTHER));
    assert_eq!(stolen.after.tapped(), Some(true));
}

#[test]
fn departing_tapped_permanent_has_no_status_and_returns_untapped_with_fresh_identity() {
    for destination in destinations()
        .into_iter()
        .filter(|destination| !matches!(destination, CardDestination::Battlefield { .. }))
    {
        let (cards, id) = seeded(CardDestination::Battlefield { controller: OWNER });
        let tapped = cards.set_tapped(cards.revision(), id, true).unwrap();
        let departed = tapped
            .next
            .apply_zone_move(tapped.next.revision(), id, destination)
            .unwrap();
        assert_eq!(departed.before, Some(tapped.after));
        assert_eq!(departed.after.tapped(), None);
        assert_ne!(departed.after.id(), id);
        assert!(departed.next.get(id).is_none());
        let returned = departed
            .next
            .apply_zone_move(
                departed.next.revision(),
                departed.after.id(),
                CardDestination::Battlefield { controller: OTHER },
            )
            .unwrap();
        assert_eq!(returned.after.tapped(), Some(false));
        assert_eq!(returned.after.owner(), OWNER);
        assert_eq!(returned.after.controller(), Some(OTHER));
        assert_ne!(returned.after.id(), id);
        assert_ne!(returned.after.id(), departed.after.id());
        assert!(returned.next.get(id).is_none());
        assert!(returned.next.get(departed.after.id()).is_none());
    }
}

#[test]
fn every_supported_zone_transition_reinitializes_only_battlefield_status() {
    for source in destinations() {
        for destination in destinations() {
            let (mut cards, id) = seeded(source);
            if matches!(source, CardDestination::Battlefield { .. }) {
                cards = cards.set_tapped(cards.revision(), id, true).unwrap().next;
            }
            let before = cards.get(id).unwrap().clone();
            let saved = cards.clone();
            let changed = cards.apply_zone_move(cards.revision(), id, destination);
            if source == destination
                && !matches!(
                    destination,
                    CardDestination::Exile | CardDestination::Command
                )
            {
                assert_eq!(changed, Err(CardIdentityError::SameZone));
            } else {
                let changed = changed.unwrap();
                assert_eq!(changed.before, Some(before));
                assert_ne!(changed.after.id(), id);
                assert_eq!(
                    changed.after.tapped(),
                    match destination {
                        CardDestination::Battlefield { .. } => Some(false),
                        _ => None,
                    }
                );
            }
            assert_eq!(cards, saved);
        }
    }
}

#[test]
fn nonpermanent_unchanged_stale_revision_and_stale_object_updates_are_atomic() {
    for destination in destinations() {
        let (cards, id) = seeded(destination);
        let saved = cards.clone();
        if matches!(destination, CardDestination::Battlefield { .. }) {
            assert_eq!(
                cards.set_tapped(cards.revision(), id, false),
                Err(CardIdentityError::StatusUnchanged)
            );
            let tapped = cards.set_tapped(cards.revision(), id, true).unwrap();
            let saved_tapped = tapped.next.clone();
            assert_eq!(
                tapped.next.set_tapped(tapped.next.revision(), id, true),
                Err(CardIdentityError::StatusUnchanged)
            );
            assert_eq!(tapped.next, saved_tapped);
        } else {
            for status in [false, true] {
                assert_eq!(
                    cards.set_tapped(cards.revision(), id, status),
                    Err(CardIdentityError::NotPermanent)
                );
            }
        }
        assert_eq!(cards, saved);
        assert_eq!(
            cards.set_tapped(cards.revision() - 1, id, true),
            Err(CardIdentityError::StaleRevision)
        );
        assert_eq!(cards, saved);
    }
    let (cards, id) = seeded(CardDestination::Battlefield { controller: OWNER });
    let departed = cards
        .apply_zone_move(cards.revision(), id, CardDestination::Hand)
        .unwrap();
    let returned = departed
        .next
        .apply_zone_move(
            departed.next.revision(),
            departed.after.id(),
            CardDestination::Battlefield { controller: OWNER },
        )
        .unwrap();
    let saved = returned.next.clone();
    assert_eq!(
        returned.next.set_tapped(returned.next.revision(), id, true),
        Err(CardIdentityError::UnknownObject)
    );
    assert_eq!(returned.next, saved);
    assert_eq!(returned.after.tapped(), Some(false));
}

#[test]
fn tap_action_never_follows_a_stable_card_key_to_a_returned_incarnation() {
    let (cards, old) = seeded(CardDestination::Battlefield { controller: OWNER });
    let hand = cards
        .apply_zone_move(cards.revision(), old, CardDestination::Hand)
        .unwrap();
    let returned = hand
        .next
        .apply_zone_move(
            hand.next.revision(),
            hand.after.id(),
            CardDestination::Battlefield { controller: OWNER },
        )
        .unwrap();
    let window = StackWindow::new(vec![OWNER, OTHER], OWNER, returned.next).unwrap();
    let window = window.finish_checks(window.revision()).unwrap();
    let saved = window.clone();
    assert_eq!(
        window.record_tap_action(window.revision(), OWNER, old),
        Err(StackWindowError::Cards(CardIdentityError::UnknownObject))
    );
    assert_eq!(window, saved);
    assert_eq!(
        window.cards().get(returned.after.id()).unwrap().tapped(),
        Some(false)
    );
    let tapped = window
        .record_tap_action(window.revision(), OWNER, returned.after.id())
        .unwrap();
    assert_eq!(
        tapped.cards().get(returned.after.id()).unwrap().tapped(),
        Some(true)
    );
    assert_eq!(
        tapped.cards().current_identity(CardId(9)),
        Some(returned.after.id())
    );
    assert!(tapped.entries().is_empty());
}
