use super::*;

#[test]
fn controller_batches_validate_all_recipients_and_roll_back_late_allocation_failure() {
    let a = SeatId(0);
    let b = SeatId(1);
    let mut cards = GameObjects::new(0);
    for id in [1, 2] {
        cards = cards
            .seed_card(
                cards.revision(),
                crate::object_identity::CardId(id),
                b,
                ObjectDestination::Battlefield { controller: b },
            )
            .unwrap()
            .next;
    }
    let objects: Vec<_> = cards.iter().map(GameObject::id).collect();
    let window = StackWindow::new(vec![a, b], a, cards).unwrap();
    let saved = window.clone();
    let invalid = BTreeMap::from([(objects[0], a), (objects[1], SeatId(99))]);
    assert_eq!(
        window.reconcile_controllers(window.revision(), &invalid),
        Err(StackWindowError::UnknownPlayer)
    );
    assert_eq!(window, saved);
    let mut exhausted = window;
    exhausted.set_test_object_revision(u64::MAX - 1);
    let saved = exhausted.clone();
    let desired = BTreeMap::from([(objects[0], a), (objects[1], a)]);
    assert_eq!(
        exhausted.reconcile_controllers(exhausted.revision(), &desired),
        Err(StackWindowError::Objects(
            ObjectIdentityError::RevisionExhausted
        ))
    );
    assert_eq!(exhausted, saved);
}
