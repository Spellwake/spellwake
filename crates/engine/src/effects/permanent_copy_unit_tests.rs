//! Trusted zone/reentry and allocator cuts not reachable through the current
//! instruction language. These are not normal whole-game reference evidence.
use super::*;
fn copy_source() -> Program {
    Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            source_cost: SourceCost::None,
            mana_cost: ManaCost::free(),
            body: body(
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::CopyTargetToSource {
                        duration: ResolvedDuration::NoStatedDuration,
                        power_toughness: None,
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: crate::life::LifeAdjustment::Gain(5.into()),
                    },
                ],
            ),
        }],
    )
}
fn announced() -> (EffectWindow, ObjectId) {
    let state = EffectWindow::new(seed(
        vec![
            card(
                1,
                copy_source(),
                ObjectDestination::Battlefield { controller: A },
            ),
            card(
                2,
                creature(),
                ObjectDestination::Battlefield { controller: A },
            ),
        ],
        vec![CardId(1), CardId(2)],
    ))
    .unwrap();
    let state = state.settle_checks(state.revision()).unwrap();
    let receiver = state.window.cards().current_identity(CardId(1)).unwrap();
    let donor = state.window.cards().current_identity(CardId(2)).unwrap();
    let state = state
        .activate_ability(
            state.revision(),
            A,
            receiver,
            ActivatedAbilityId(1),
            TargetRef::Object(donor),
            None,
        )
        .unwrap()
        .into_state();
    (state, receiver)
}
fn ready(state: EffectWindow) -> EffectWindow {
    let state = state.settle_checks(state.revision()).unwrap();
    let state = state.pass(state.revision(), A).unwrap();
    let state = state.settle_checks(state.revision()).unwrap();
    state.pass(state.revision(), B).unwrap()
}
#[test]
fn old_activated_source_does_not_follow_a_departed_returned_receiver() {
    let (state, old) = announced();
    let moved = state
        .qualification_zone_moves(state.revision(), &[(old, ObjectDestination::Hand)])
        .unwrap()
        .into_state();
    let hand = moved.window.cards().current_identity(CardId(1)).unwrap();
    let returned = moved
        .qualification_zone_moves(
            moved.revision(),
            &[(hand, ObjectDestination::Battlefield { controller: A })],
        )
        .unwrap()
        .into_state();
    let new = returned.window.cards().current_identity(CardId(1)).unwrap();
    assert_ne!(old, new);
    let state = ready(returned);
    let next_effect = state.characteristics.next_effect_sequence();
    let next_timestamp = state.characteristics.next_timestamp_sequence();
    let saved = state.clone();
    let completed = state.resolve_top(state.revision()).unwrap();
    assert_eq!(completed.life_total(A), Some(&25.into()));
    assert_eq!(
        completed.characteristics.next_effect_sequence(),
        next_effect
    );
    assert_eq!(
        completed.characteristics.next_timestamp_sequence(),
        next_timestamp
    );
    assert_eq!(completed.copiable_program(new), Some(&copy_source()));
    assert!(completed.continuous_effects().next().is_none());
    assert_eq!(state, saved);
}
#[test]
fn copy_allocation_failure_discards_the_entire_tentative_resolution() {
    let (state, _) = announced();
    let state = ready(state);
    for timestamp in [false, true] {
        let mut broken = state.clone();
        if timestamp {
            broken
                .characteristics
                .qualification_set_next_timestamp(u64::MAX);
        } else {
            broken
                .characteristics
                .qualification_set_next_effect(u64::MAX);
        }
        let saved = broken.clone();
        assert!(broken.resolve_top(broken.revision()).is_err());
        assert_eq!(broken, saved);
        assert_eq!(broken.life_total(A), Some(&20.into()));
        assert!(broken.continuous_effects().next().is_none());
    }
}
