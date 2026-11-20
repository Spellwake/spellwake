use super::*;
use crate::object_identity::{CardId, GameObjects, ObjectDestination, ObjectZone};

const A: SeatId = SeatId(13);
const B: SeatId = SeatId(2);
const C: SeatId = SeatId(89);

fn cards() -> GameObjects {
    let mut cards = GameObjects::new(0);
    for (card, controller) in [(1, A), (2, A), (11, B), (12, B), (21, C)] {
        cards = cards
            .seed_card(
                cards.revision(),
                CardId(card),
                controller,
                ObjectDestination::Battlefield { controller },
            )
            .unwrap()
            .next;
    }
    cards.record_turn_start(cards.revision(), A).unwrap()
}

/// Administrative fixture lookup; combat always retains the resulting incarnation.
fn object(cards: &GameObjects, card: u64) -> ObjectId {
    cards.current_identity(CardId(card)).unwrap()
}

fn participant(cards: &GameObjects, object: ObjectId) -> Combatant {
    Combatant::from_object(cards.get(object).unwrap()).unwrap()
}

fn attacks(cards: &GameObjects, selected: &[(u64, SeatId)]) -> CombatState {
    CombatState::new(
        selected
            .iter()
            .map(|&(card, defender)| (participant(cards, object(cards, card)), defender))
            .collect(),
    )
    .unwrap()
}

/// These fixtures represent already-established creatures. This closure tests
/// canonical identity/control intervals, not full creature/type/layer integration.
fn reconcile(combat: &mut CombatState, cards: &GameObjects) {
    combat.reconcile(|member| {
        cards.get(member.object()).is_some_and(|card| {
            card.zone() == ObjectZone::Battlefield
                && card.controller() == Some(member.controller())
                && card.control_epoch() == Some(member.control_epoch())
        })
    });
}

fn move_card(cards: GameObjects, object: ObjectId, destination: ObjectDestination) -> GameObjects {
    cards
        .apply_zone_move(cards.revision(), object, destination)
        .unwrap()
        .require_moved()
        .unwrap()
        .next
}

#[test]
fn multiple_blockers_share_one_attacker_while_another_is_unblocked() {
    let cards = cards();
    let first = object(&cards, 1);
    let second = object(&cards, 2);
    let blockers = [object(&cards, 11), object(&cards, 12)];
    let mut combat = attacks(&cards, &[(1, B), (2, B)]);
    combat
        .declare_blocks(
            B,
            blockers
                .map(|blocker| (participant(&cards, blocker), first))
                .to_vec(),
        )
        .unwrap();
    assert_eq!(combat.attackers().count(), 2);
    assert_eq!(combat.blockers().count(), 2);
    assert_eq!(
        combat.attacker(first).unwrap().block_status(),
        BlockStatus::Blocked
    );
    assert_eq!(
        combat.attacker(second).unwrap().block_status(),
        BlockStatus::Unblocked
    );
    for blocker in blockers {
        let member = combat.blocker(blocker).unwrap();
        assert_eq!(member.combatant().controller(), B);
        assert_eq!(member.attacker(), Some(first));
    }
    assert!(combat.had_attackers());
}

#[test]
fn removing_every_blocker_preserves_the_surviving_attackers_blocked_status() {
    let mut cards = cards();
    let attacker = object(&cards, 1);
    let blockers = [object(&cards, 11), object(&cards, 12)];
    let mut combat = attacks(&cards, &[(1, B)]);
    combat
        .declare_blocks(
            B,
            blockers
                .map(|blocker| (participant(&cards, blocker), attacker))
                .to_vec(),
        )
        .unwrap();
    for (index, blocker) in blockers.into_iter().enumerate() {
        cards = move_card(cards, blocker, ObjectDestination::Hand);
        reconcile(&mut combat, &cards);
        assert!(combat.blocker(blocker).is_none());
        assert_eq!(combat.blockers().count(), 1 - index);
        assert_eq!(
            combat.attacker(attacker).unwrap().block_status(),
            BlockStatus::Blocked
        );
        assert!(combat.had_attackers());
    }
    assert_eq!(combat.attackers().count(), 1);
}

#[test]
fn removing_attacker_keeps_blocking_membership_without_an_obsolete_assignment() {
    let mut cards = cards();
    let attacker = object(&cards, 1);
    let blocker = object(&cards, 11);
    let mut combat = attacks(&cards, &[(1, B)]);
    combat
        .declare_blocks(B, vec![(participant(&cards, blocker), attacker)])
        .unwrap();
    cards = move_card(cards, attacker, ObjectDestination::Graveyard);
    reconcile(&mut combat, &cards);
    assert!(combat.attacker(attacker).is_none());
    assert_eq!(combat.attackers().count(), 0);
    assert_eq!(combat.blockers().count(), 1);
    assert_eq!(combat.blocker(blocker).unwrap().attacker(), None);
    assert_eq!(
        combat.blocker(blocker).unwrap().combatant().object(),
        blocker
    );
    assert!(combat.had_attackers());
    reconcile(&mut combat, &cards);
    assert_eq!(combat.blocker(blocker).unwrap().attacker(), None);
    assert!(combat.had_attackers());
}

#[test]
fn same_controller_reaffirmation_and_tapping_do_not_remove_combatants() {
    let mut cards = cards();
    let attacker = object(&cards, 1);
    let blocker = object(&cards, 11);
    let mut combat = attacks(&cards, &[(1, B)]);
    combat
        .declare_blocks(B, vec![(participant(&cards, blocker), attacker)])
        .unwrap();
    let accepted = combat.clone();
    for (member, controller) in [(attacker, A), (blocker, B)] {
        let epoch = cards.get(member).unwrap().control_epoch();
        cards = cards
            .apply_controller(cards.revision(), member, controller)
            .unwrap()
            .next;
        assert_eq!(cards.get(member).unwrap().control_epoch(), epoch);
        reconcile(&mut combat, &cards);
        assert_eq!(combat, accepted);
        for tapped in [true, false] {
            cards = cards
                .set_tapped(cards.revision(), member, tapped)
                .unwrap()
                .next;
            reconcile(&mut combat, &cards);
            assert_eq!(combat, accepted);
            assert_eq!(cards.get(member).unwrap().control_epoch(), epoch);
        }
    }
}

#[test]
fn control_away_and_back_removes_only_the_changed_control_interval() {
    for change_attacker in [true, false] {
        let mut cards = cards();
        let attacker = object(&cards, 1);
        let blocker = object(&cards, 11);
        let mut combat = attacks(&cards, &[(1, B)]);
        combat
            .declare_blocks(B, vec![(participant(&cards, blocker), attacker)])
            .unwrap();
        let (changed, original_controller) = if change_attacker {
            (attacker, A)
        } else {
            (blocker, B)
        };
        let epoch = cards.get(changed).unwrap().control_epoch();
        for controller in [C, original_controller] {
            cards = cards
                .apply_controller(cards.revision(), changed, controller)
                .unwrap()
                .next;
        }
        assert_eq!(cards.get(changed).unwrap().id(), changed);
        assert_eq!(
            cards.get(changed).unwrap().controller(),
            Some(original_controller)
        );
        assert_ne!(cards.get(changed).unwrap().control_epoch(), epoch);
        reconcile(&mut combat, &cards);
        if change_attacker {
            assert!(combat.attacker(attacker).is_none());
            assert_eq!(combat.blocker(blocker).unwrap().attacker(), None);
        } else {
            assert!(combat.blocker(blocker).is_none());
            assert_eq!(
                combat.attacker(attacker).unwrap().block_status(),
                BlockStatus::Blocked
            );
        }
        assert!(combat.had_attackers());
    }
}

#[test]
fn returned_cards_do_not_rejoin_combat_through_their_stable_card_ids() {
    let mut cards = cards();
    let attacker = object(&cards, 1);
    let blocker = object(&cards, 11);
    let mut combat = attacks(&cards, &[(1, B)]);
    combat
        .declare_blocks(B, vec![(participant(&cards, blocker), attacker)])
        .unwrap();
    for (card, old_object, controller) in [(1, attacker, A), (11, blocker, B)] {
        cards = move_card(cards, old_object, ObjectDestination::Hand);
        let in_hand = object(&cards, card);
        cards = move_card(
            cards,
            in_hand,
            ObjectDestination::Battlefield { controller },
        );
        let returned = object(&cards, card);
        assert_ne!(returned, old_object);
        assert_ne!(returned, in_hand);
        assert_eq!(cards.get(returned).unwrap().controller(), Some(controller));
    }
    reconcile(&mut combat, &cards);
    assert_eq!(combat.attackers().count(), 0);
    assert_eq!(combat.blockers().count(), 0);
    assert!(combat.attacker(object(&cards, 1)).is_none());
    assert!(combat.blocker(object(&cards, 11)).is_none());
    assert!(combat.had_attackers());
}

#[test]
fn invalid_block_declarations_preserve_all_prior_defender_choices() {
    let cards = cards();
    let first = object(&cards, 1);
    let second = object(&cards, 2);
    let b1 = participant(&cards, object(&cards, 11));
    let b2 = participant(&cards, object(&cards, 12));
    let c1 = participant(&cards, object(&cards, 21));
    let mut combat = attacks(&cards, &[(1, B), (2, C)]);
    for (selected, error) in [
        (
            vec![(b1, first), (b1, first)],
            CombatError::DuplicateBlocker,
        ),
        (vec![(b1, first), (c1, first)], CombatError::IllegalBlocker),
        (
            vec![(b1, first), (b2, second)],
            CombatError::IllegalBlockTarget,
        ),
        (
            vec![(b1, first), (b2, b1.object())],
            CombatError::IllegalBlockTarget,
        ),
    ] {
        let before = combat.clone();
        assert_eq!(combat.declare_blocks(B, selected), Err(error));
        assert_eq!(combat, before);
        assert_eq!(combat.blockers().count(), 0);
        assert_eq!(
            combat.attacker(first).unwrap().block_status(),
            BlockStatus::NotDeclared
        );
    }
    combat.declare_blocks(B, vec![(b1, first)]).unwrap();
    let accepted_b = combat.clone();
    assert_eq!(
        combat.declare_blocks(C, vec![(c1, first)]),
        Err(CombatError::IllegalBlockTarget)
    );
    assert_eq!(combat, accepted_b);
    assert_eq!(
        combat.declare_blocks(C, vec![(b2, second)]),
        Err(CombatError::IllegalBlocker)
    );
    assert_eq!(combat, accepted_b);
    combat.declare_blocks(C, vec![]).unwrap();
    assert_eq!(
        combat.attacker(first).unwrap().block_status(),
        BlockStatus::Blocked
    );
    assert_eq!(
        combat.attacker(second).unwrap().block_status(),
        BlockStatus::Unblocked
    );
    assert_eq!(combat.blocker(b1.object()).unwrap().attacker(), Some(first));
    let accepted_all = combat.clone();
    assert_eq!(
        combat.declare_blocks(B, vec![]),
        Err(CombatError::AlreadyDeclared)
    );
    assert_eq!(
        combat.declare_blocks(C, vec![]),
        Err(CombatError::AlreadyDeclared)
    );
    assert_eq!(combat, accepted_all);
}

#[test]
fn duplicate_attackers_reject_and_a_true_empty_combat_has_no_attack_history() {
    let cards = cards();
    let attacker = participant(&cards, object(&cards, 1));
    let before = cards.clone();
    assert_eq!(
        CombatState::new(vec![(attacker, B), (attacker, C)]),
        Err(CombatError::DuplicateAttacker)
    );
    assert_eq!(cards, before);
    let mut empty = CombatState::new(vec![]).unwrap();
    reconcile(&mut empty, &cards);
    assert_eq!(empty.attackers().count(), 0);
    assert_eq!(empty.blockers().count(), 0);
    assert!(!empty.had_attackers());
}

#[test]
fn unrestricted_opponents_follow_canonical_apnap_order_with_irregular_seat_ids() {
    let seats = [SeatId(31), SeatId(7), SeatId(80), SeatId(2)];
    let rules = CombatRules::UnrestrictedFreeForAll;
    for (player, expected) in [
        (SeatId(31), vec![SeatId(7), SeatId(80), SeatId(2)]),
        (SeatId(7), vec![SeatId(80), SeatId(2), SeatId(31)]),
        (SeatId(80), vec![SeatId(2), SeatId(31), SeatId(7)]),
        (SeatId(2), vec![SeatId(31), SeatId(7), SeatId(80)]),
    ] {
        assert_eq!(rules.opponents(&seats, player), Ok(expected));
    }
    assert_eq!(
        rules.opponents(&seats, SeatId(99)),
        Err(CombatError::UnknownPlayer)
    );
    assert_eq!(rules.opponents(&[], A), Err(CombatError::UnknownPlayer));
}
