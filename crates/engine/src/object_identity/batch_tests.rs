//! Real batch registry invariants; no interpreted-card or format claim.
use super::*;
fn definition() -> (
    spellwake_contracts::creature_types::CreatureTypeVocabulary,
    Program,
) {
    let v = crate::test_support::creature_types::selected_vocabulary().unwrap();
    let p = crate::token::TokenDefinition::new(
        Program::artifact(None, vec![]),
        crate::token::TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap()
    .materialize(&v)
    .unwrap();
    (v.as_ref().clone(), p)
}
#[test]
fn token_batch_receipts_are_actual_ordered_objects_without_physical_indexes() {
    let (v, p) = definition();
    let base = GameObjects::new(0);
    let created = base
        .insert_tokens(0, vec![(SeatId(1), p.clone()), (SeatId(0), p)], &v)
        .unwrap();
    assert_eq!(created.objects.len(), 2);
    assert_ne!(created.objects[0].id(), created.objects[1].id());
    assert_eq!(
        created
            .objects
            .iter()
            .map(GameObject::owner)
            .collect::<Vec<_>>(),
        vec![SeatId(1), SeatId(0)]
    );
    assert_eq!(created.next.revision(), 2);
    assert_eq!(created.next.next_identity_sequence(), 3);
    assert!(created.next.by_card.is_empty());
    assert!(created.next.ordered_zones.is_empty());
    assert_eq!(base, GameObjects::new(0));
}
#[test]
fn bad_later_token_and_allocator_overflow_reject_the_whole_batch() {
    let (v, p) = definition();
    let invalid = Program::artifact(None, vec![]);
    let base = GameObjects::new(0);
    let saved = base.clone();
    assert!(
        base.insert_tokens(0, vec![(SeatId(0), p.clone()), (SeatId(0), invalid)], &v)
            .is_err()
    );
    assert_eq!(base, saved);
    for identity in [true, false] {
        let mut base = saved.clone();
        if identity {
            base.next_identity = u64::MAX - 1;
        } else {
            base.revision = u64::MAX - 1;
        }
        let before = base.clone();
        assert!(
            base.insert_tokens(
                base.revision(),
                vec![(SeatId(0), p.clone()), (SeatId(0), p.clone())],
                &v
            )
            .is_err()
        );
        assert_eq!(base, before);
    }
}
#[test]
fn empty_token_batch_consumes_no_revision_or_identity_even_at_exhaustion() {
    let (v, _) = definition();
    let mut base = GameObjects::new(u64::MAX);
    base.next_identity = u64::MAX;
    assert_eq!(base.insert_tokens(u64::MAX, vec![], &v).unwrap().next, base);
    assert_eq!(
        base.insert_tokens(u64::MAX - 1, vec![], &v),
        Err(ObjectIdentityError::StaleRevision)
    );
}
