use super::*;

#[test]
fn derived_control_keeps_the_default_and_stack_caster_fact_through_permanent_entry() {
    let a = SeatId(0);
    let b = SeatId(1);
    let seeded = GameObjects::new(0)
        .seed_card(0, CardId(1), a, ObjectDestination::Stack { controller: a })
        .unwrap();
    let changed = seeded
        .next
        .apply_effective_controller(seeded.next.revision(), seeded.after.id(), b)
        .unwrap();
    assert_eq!(changed.after.controller(), Some(b));
    assert_eq!(changed.after.default_controller(), Some(a));
    assert_eq!(changed.after.control_epoch(), None);
    let entered = changed
        .next
        .apply_zone_move(
            changed.next.revision(),
            changed.after.id(),
            ObjectDestination::Battlefield { controller: b },
        )
        .unwrap()
        .require_moved()
        .unwrap();
    assert_eq!(entered.after.controller(), Some(b));
    assert_eq!(entered.after.default_controller(), Some(a));
    assert_eq!(
        entered.after.has_continuous_control_since_turn_start(),
        Some(false)
    );
    let left = entered
        .next
        .apply_zone_move(
            entered.next.revision(),
            entered.after.id(),
            ObjectDestination::Hand,
        )
        .unwrap()
        .require_moved()
        .unwrap();
    assert_eq!(left.after.default_controller(), None);
}
