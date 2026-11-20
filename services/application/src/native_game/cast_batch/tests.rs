//! Atomic adapter mechanisms using an explicit genuine vocabulary; not full-card qualification.
use super::*;
use crate::{
    native_game::tests::fixture::*,
    player_wire::{ManaColor, ManaSpend, PlayerPayment},
};
use spellwake_engine::{
    effects::Program,
    land::BasicLandType,
    mana::{ManaAbilityRef, ManaCost, ManaType},
    object_identity::ObjectDestination,
};
fn fixture() -> GameState {
    main_state(vec![
        card(
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::land(
                BTreeSet::from([spellwake_engine::land::LandType::Forest]),
                vec![],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::land(
                BTreeSet::from([spellwake_engine::land::LandType::Forest]),
                vec![],
            ),
        ),
        card(
            3,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::specific(ManaType::Green, 2.into())),
                2.into(),
                2.into(),
                BTreeSet::new(),
            ),
        ),
    ])
}
fn inputs(scope: &mut NativeScope<'_>, source: &GameState) -> Vec<PlayerCommand> {
    let mut actions = Vec::new();
    for id in [1, 2] {
        let object = object(source, id);
        let source = scope.issue(NativeReference::Object(object)).unwrap();
        let ability = scope
            .issue(NativeReference::ManaAbility {
                source: object,
                ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
            })
            .unwrap();
        actions.push(PlayerAction::ActivateMana { source, ability });
    }
    let spell = scope
        .issue(NativeReference::Object(object(source, 3)))
        .unwrap();
    actions.push(PlayerAction::CastUntargeted {
        source: spell,
        payment: Some(PlayerPayment {
            spend: vec![ManaSpend {
                color: ManaColor::Green,
                amount: crate::player_wire::RulesCount::try_from("2".to_owned()).unwrap(),
            }],
        }),
    });
    actions
        .into_iter()
        .map(|action| PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: scope.basis().clone(),
            action,
        })
        .collect()
}
#[test]
fn accepted_batch_matches_sequential_original_codec_replay_and_preserves_source() {
    let source = fixture();
    let before = source.clone();
    let mut scope = scope(&source, viewer_a());
    let originals = inputs(&mut scope, &source);
    let batch = scope.prepare_cast_batch(&originals).unwrap();
    assert_eq!(source, before);
    assert_eq!(batch.state().basis().revision, source.basis().revision + 3);
    let (final_state, records) = batch.into_parts();
    assert_eq!(records.len(), 3);
    let mut restored = source.clone();
    for bytes in records {
        restored = restore_input(&restored, binding(&restored, viewer_a()), &bytes)
            .unwrap()
            .into_parts()
            .3
            .into_state();
    }
    assert_eq!(restored, final_state);
}
#[test]
fn late_cast_failure_drops_all_staged_mana_without_mutating_original() {
    let source = fixture();
    let before = source.clone();
    let mut scope = scope(&source, viewer_a());
    let mut originals = inputs(&mut scope, &source);
    if let PlayerAction::CastUntargeted { payment, .. } = &mut originals[2].action {
        *payment = Some(PlayerPayment {
            spend: vec![ManaSpend {
                color: ManaColor::Green,
                amount: crate::player_wire::RulesCount::try_from("3".to_owned()).unwrap(),
            }],
        });
    }
    assert!(matches!(
        scope.prepare_cast_batch(&originals),
        Err(NativeCastBatchError::Bridge { step: 2, .. })
    ));
    assert_eq!(source, before);
    assert!(
        scope
            .prepare_cast_batch(&inputs(
                &mut crate::native_game::tests::fixture::scope(&source, viewer_a()),
                &source
            ))
            .is_err()
    ); // Different scope handles do not authorize a batch.
}
#[test]
fn reused_mana_source_rejects_the_whole_batch() {
    let source = fixture();
    let before = source.clone();
    let mut scope = scope(&source, viewer_a());
    let mut originals = inputs(&mut scope, &source);
    originals[1].action = originals[0].action.clone();
    assert!(matches!(
        scope.prepare_cast_batch(&originals),
        Err(NativeCastBatchError::Bridge { step: 1, .. })
    ));
    assert_eq!(source, before);
}
#[test]
fn stale_basis_duplicate_request_and_unsupported_action_order_are_rejected() {
    let source = fixture();
    let mut scope = scope(&source, viewer_a());
    let originals = inputs(&mut scope, &source);
    let mut stale = originals.clone();
    stale[1].basis.revision = 999u64.into();
    assert!(matches!(
        scope.prepare_cast_batch(&stale),
        Err(NativeCastBatchError::InvalidShape)
    ));
    let mut duplicate = originals.clone();
    duplicate[1].request_id = duplicate[0].request_id;
    assert!(matches!(
        scope.prepare_cast_batch(&duplicate),
        Err(NativeCastBatchError::InvalidShape)
    ));
    let mut wrong = originals.clone();
    wrong[0].action = PlayerAction::Pass {};
    assert!(matches!(
        scope.prepare_cast_batch(&wrong),
        Err(NativeCastBatchError::InvalidShape)
    ));
    assert!(matches!(
        scope.prepare_cast_batch(&[]),
        Err(NativeCastBatchError::InvalidShape)
    ));
}
#[test]
fn no_mana_prefix_is_allowed_when_pool_already_pays() {
    let source = fixture();
    let mut scope = scope(&source, viewer_a());
    let originals = inputs(&mut scope, &source);
    // A zero-prefix paidcast against an empty pool is a real native rejection, not a shape error.
    assert!(matches!(
        scope.prepare_cast_batch(&originals[2..]),
        Err(NativeCastBatchError::Bridge { step: 0, .. })
    ));
}
#[test]
fn required_mana_color_choice_interrupts_batch_without_committing_prefix() {
    use spellwake_engine::mana::{ManaAbility, ManaAbilityId, ManaOutput};
    let source = main_state(vec![
        card(
            1,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::land(
                BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors: BTreeSet::from([ManaType::Green, ManaType::Blue]),
                    },
                }],
            ),
        ),
        card(
            3,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(ManaCost::one(ManaType::Green)),
                2.into(),
                2.into(),
                BTreeSet::new(),
            ),
        ),
    ]);
    let before = source.clone();
    let mut view = scope(&source, viewer_a());
    let land = view
        .issue(NativeReference::Object(object(&source, 1)))
        .unwrap();
    let ability = view
        .issue(NativeReference::ManaAbility {
            source: object(&source, 1),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        })
        .unwrap();
    let spell = view
        .issue(NativeReference::Object(object(&source, 3)))
        .unwrap();
    let original = [
        command(
            &view,
            PlayerAction::ActivateMana {
                source: land,
                ability,
            },
        ),
        command(
            &view,
            PlayerAction::CastUntargeted {
                source: spell,
                payment: Some(PlayerPayment {
                    spend: vec![ManaSpend {
                        color: ManaColor::Green,
                        amount: crate::player_wire::RulesCount::try_from("1".to_owned()).unwrap(),
                    }],
                }),
            },
        ),
    ];
    assert!(matches!(
        view.prepare_cast_batch(&original),
        Err(NativeCastBatchError::Boundary { step: 1 })
    ));
    assert_eq!(source, before);
}
#[test]
fn a_foreign_priority_holder_cannot_prepare_a_batch() {
    let source = fixture();
    let mut a = scope(&source, viewer_a());
    let originals = inputs(&mut a, &source);
    let foreign = scope(&source, viewer_b());
    let actions = originals
        .iter()
        .map(|original| PlayerCommand {
            request_id: Uuid::new_v4(),
            basis: foreign.basis().clone(),
            action: original.action.clone(),
        })
        .collect::<Vec<_>>();
    // A foreign scope cannot import the acting participant's issued tokens.
    assert!(matches!(
        foreign.prepare_cast_batch(&actions),
        Err(NativeCastBatchError::Boundary { step: 0 })
    ));
    assert_eq!(foreign.basis(), a.basis());
}
#[test]
fn zero_and_eight_mana_prefix_limits_are_real_native_paths() {
    for count in [0usize, 8] {
        let mut cards = Vec::new();
        for id in 1..=count {
            cards.push(card(
                id as u64,
                A,
                ObjectDestination::Battlefield { controller: A },
                Program::land(
                    BTreeSet::from([spellwake_engine::land::LandType::Forest]),
                    vec![],
                ),
            ));
        }
        cards.push(card(
            99,
            A,
            ObjectDestination::Hand,
            Program::creature(
                Some(if count == 0 {
                    ManaCost::free()
                } else {
                    ManaCost::specific(ManaType::Green, 8.into())
                }),
                2.into(),
                2.into(),
                BTreeSet::new(),
            ),
        ));
        let source = main_state(cards);
        let mut view = scope(&source, viewer_a());
        let mut originals = Vec::new();
        for id in 1..=count {
            let o = object(&source, id as u64);
            let source = view.issue(NativeReference::Object(o)).unwrap();
            let ability = view
                .issue(NativeReference::ManaAbility {
                    source: o,
                    ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
                })
                .unwrap();
            originals.push(command(
                &view,
                PlayerAction::ActivateMana { source, ability },
            ));
        }
        let spell = view
            .issue(NativeReference::Object(object(&source, 99)))
            .unwrap();
        originals.push(command(
            &view,
            PlayerAction::CastUntargeted {
                source: spell,
                payment: if count == 0 {
                    None
                } else {
                    Some(PlayerPayment {
                        spend: vec![ManaSpend {
                            color: ManaColor::Green,
                            amount: crate::player_wire::RulesCount::try_from("8".to_owned())
                                .unwrap(),
                        }],
                    })
                },
            },
        ));
        assert_eq!(
            view.prepare_cast_batch(&originals)
                .unwrap()
                .into_parts()
                .1
                .len(),
            count + 1
        );
        originals.push(originals[0].clone());
        assert!(view.prepare_cast_batch(&originals).is_err());
    }
}
