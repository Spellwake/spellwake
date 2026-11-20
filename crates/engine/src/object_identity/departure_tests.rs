//! Registry cuts only: no player departure authorization or complete CR800.4 cleanup.
use super::*;
use crate::test_support::creature_types::selected_vocabulary;
use spellwake_contracts::names::{EnglishCardName, ObjectNames};

const LEAVER: SeatId = SeatId(0);
const OTHER_LEAVER: SeatId = SeatId(1);
const SURVIVOR: SeatId = SeatId(2);

fn populated() -> GameObjects {
    let mut registry = GameObjects::new(0);
    let mut card = 0;
    for owner in [LEAVER, SURVIVOR, OTHER_LEAVER] {
        for destination in [
            ObjectDestination::LibraryBottom,
            ObjectDestination::Hand,
            ObjectDestination::Graveyard,
            ObjectDestination::Battlefield {
                controller: if owner == SURVIVOR { LEAVER } else { SURVIVOR },
            },
            ObjectDestination::Stack {
                controller: SURVIVOR,
            },
            ObjectDestination::Exile,
            ObjectDestination::Command,
            ObjectDestination::LibraryBottom,
            ObjectDestination::Graveyard,
        ] {
            card += 1;
            registry = registry
                .seed_card(registry.revision(), CardId(card), owner, destination)
                .unwrap()
                .next;
        }
    }
    let vocabulary = selected_vocabulary().unwrap();
    let mut definition = Program::creature(None, 1.into(), 1.into(), BTreeSet::new());
    definition.names = ObjectNames::single(EnglishCardName::new("Departure token".into()).unwrap());
    for owner in [LEAVER, SURVIVOR] {
        for destination in [
            None,
            Some(ObjectDestination::LibraryBottom),
            Some(ObjectDestination::Hand),
            Some(ObjectDestination::Graveyard),
            Some(ObjectDestination::Stack {
                controller: SURVIVOR,
            }),
            Some(ObjectDestination::Exile),
            Some(ObjectDestination::Command),
        ] {
            let inserted = registry
                .insert_token(registry.revision(), owner, definition.clone(), &vocabulary)
                .unwrap();
            let object = inserted.after.id();
            registry = inserted.next;
            if let Some(destination) = destination {
                registry = registry
                    .apply_zone_move(registry.revision(), object, destination)
                    .unwrap()
                    .next;
            }
        }
    }
    registry
}

#[test]
fn departure_removes_all_owned_card_and_token_zones_without_moving_survivor_objects() {
    let registry = populated();
    let before = registry.clone();
    let owners = BTreeSet::from([OTHER_LEAVER, LEAVER]);
    let expected: Vec<_> = registry
        .iter()
        .filter(|object| owners.contains(&object.owner()))
        .cloned()
        .collect();
    let survivors: Vec<_> = registry
        .iter()
        .filter(|object| object.owner() == SURVIVOR)
        .cloned()
        .collect();
    let receipt = registry
        .remove_owned_objects(registry.revision(), &owners)
        .unwrap();
    assert_eq!(registry, before);
    assert_eq!(receipt.objects, expected);
    assert!(
        receipt
            .objects
            .windows(2)
            .all(|pair| pair[0].id() < pair[1].id())
    );
    assert_eq!(receipt.next.iter().cloned().collect::<Vec<_>>(), survivors);
    assert!(
        receipt
            .next
            .iter()
            .any(|object| object.owner() == SURVIVOR && object.controller() == Some(LEAVER))
    );
    assert_eq!(
        receipt.next.next_identity_sequence(),
        registry.next_identity_sequence()
    );
    assert_eq!(
        receipt.next.revision(),
        registry.revision() + u64::try_from(expected.len()).unwrap()
    );
    assert_eq!(receipt.next.library(SURVIVOR), registry.library(SURVIVOR));
    assert_eq!(
        receipt.next.graveyard(SURVIVOR),
        registry.graveyard(SURVIVOR)
    );
    for object in &expected {
        assert!(receipt.next.get(object.id()).is_none());
        if let Some(card) = object.card_id() {
            assert_eq!(receipt.next.current_identity(card), None);
        }
    }
    for owner in owners {
        assert!(receipt.next.library(owner).is_empty());
        assert!(receipt.next.graveyard(owner).is_empty());
    }
    // A new birth uses the old unconsumed identity rather than recycling one.
    let inserted = receipt
        .next
        .seed_card(
            receipt.next.revision(),
            CardId(1000),
            SURVIVOR,
            ObjectDestination::Hand,
        )
        .unwrap();
    assert_eq!(
        inserted.after.id(),
        ObjectId(registry.next_identity_sequence())
    );
}

#[test]
fn departure_empty_or_absent_owner_is_a_noop_at_max_revision_but_never_ignores_stale_basis() {
    let mut registry = populated();
    registry.revision = u64::MAX;
    for owners in [BTreeSet::new(), BTreeSet::from([SeatId(99)])] {
        let receipt = registry.remove_owned_objects(u64::MAX, &owners).unwrap();
        assert_eq!(receipt.next, registry);
        assert!(receipt.objects.is_empty());
        assert_eq!(
            registry.remove_owned_objects(u64::MAX - 1, &owners),
            Err(ObjectIdentityError::StaleRevision)
        );
    }
}

#[test]
fn departure_revision_capacity_is_checked_for_the_whole_batch_and_rejections_are_atomic() {
    let mut registry = populated();
    let owners = BTreeSet::from([LEAVER]);
    let count = u64::try_from(
        registry
            .iter()
            .filter(|object| object.owner() == LEAVER)
            .count(),
    )
    .unwrap();
    registry.revision = u64::MAX - count + 1;
    let before = registry.clone();
    assert_eq!(
        registry.remove_owned_objects(registry.revision(), &owners),
        Err(ObjectIdentityError::RevisionExhausted)
    );
    assert_eq!(registry, before);
    registry.revision -= 1;
    assert_eq!(
        registry
            .remove_owned_objects(registry.revision(), &owners)
            .unwrap()
            .next
            .revision(),
        u64::MAX
    );
}

#[test]
fn departure_rejects_bidirectional_card_and_pile_corruption_including_survivor_indexes() {
    let registry = populated();
    let survivor = registry
        .iter()
        .find(|object| {
            object.owner() == SURVIVOR
                && matches!(object.zone(), ObjectZone::Library(_))
                && object.card_id().is_some()
        })
        .unwrap();
    let token = registry
        .iter()
        .find(|object| object.owner() == SURVIVOR && object.is_token())
        .unwrap();
    for case in 0..14 {
        let mut bad = registry.clone();
        match case {
            0 => {
                bad.by_card.remove(&survivor.card_id().unwrap());
            }
            1 => {
                bad.by_card.insert(CardId(999), survivor.id());
            }
            2 => {
                bad.by_card.insert(CardId(999), ObjectId(u64::MAX));
            }
            3 => {
                bad.ordered_zones
                    .get_mut(&OrderedZone::Library(SURVIVOR))
                    .unwrap()
                    .retain(|id| *id != survivor.id());
            }
            4 => {
                bad.ordered_zones
                    .get_mut(&OrderedZone::Library(SURVIVOR))
                    .unwrap()
                    .push(survivor.id());
            }
            5 => {
                bad.ordered_zones
                    .entry(OrderedZone::Graveyard(SURVIVOR))
                    .or_default()
                    .push(survivor.id());
            }
            6 => {
                bad.ordered_zones
                    .entry(OrderedZone::Library(SURVIVOR))
                    .or_default()
                    .push(token.id());
            }
            7 => {
                bad.by_card.insert(CardId(999), token.id());
            }
            8 => {
                let object = bad.current.remove(&survivor.id()).unwrap();
                bad.current.insert(ObjectId(u64::MAX), object);
            }
            9 | 11 => {
                let from = if case == 9 {
                    OrderedZone::Library(SURVIVOR)
                } else {
                    OrderedZone::Graveyard(SURVIVOR)
                };
                let id = bad.ordered_zones[&from][0];
                bad.ordered_zones
                    .get_mut(&from)
                    .unwrap()
                    .retain(|other| *other != id);
                let to = if case == 9 {
                    OrderedZone::Library(LEAVER)
                } else {
                    OrderedZone::Graveyard(LEAVER)
                };
                bad.ordered_zones.entry(to).or_default().push(id);
                bad.current.get_mut(&id).unwrap().zone = if case == 9 {
                    ObjectZone::Library(LEAVER)
                } else {
                    ObjectZone::Graveyard(LEAVER)
                };
            }
            10 | 12 => {
                let id = bad
                    .current
                    .values()
                    .find(|object| {
                        object.owner() == SURVIVOR
                            && object.zone() == ObjectZone::Hand(SURVIVOR)
                            && object.is_token() == (case == 12)
                    })
                    .unwrap()
                    .id();
                bad.current.get_mut(&id).unwrap().zone = ObjectZone::Hand(LEAVER);
            }
            13 => {
                bad.ordered_zones
                    .entry(OrderedZone::Library(SURVIVOR))
                    .or_default()
                    .push(ObjectId(u64::MAX));
            }
            _ => unreachable!(),
        }
        let before = bad.clone();
        for owners in [
            BTreeSet::from([LEAVER]),
            BTreeSet::new(),
            BTreeSet::from([SeatId(99)]),
        ] {
            assert_eq!(
                bad.remove_owned_objects(bad.revision(), &owners),
                Err(ObjectIdentityError::InconsistentRemoval),
                "accepted corruption {case} with {owners:?}"
            );
        }
        assert_eq!(bad, before);
    }
}
