//! Bridge qualification scopes are explicit: basis shape versus opt-in real kernel composition.
use super::*;
use crate::player_wire::{
    ManaColor, ManaSpend, PlayerAction, PlayerPayment, PlayerTarget, RulesCount,
};
use spellwake_contracts::{creature_types::CreatureVocabularyId, names::NameRules};
use spellwake_engine::{
    activated::{ActivatedAbility, ActivatedAbilityId, SourceCost},
    effects::Program,
    game::{GameCommand, GameContext, GameId},
    land::{BasicLandType, LandType},
    life::LifeAdjustment,
    mana::{ManaAbility, ManaAbilityId, ManaAbilityRef, ManaCost, ManaOutput, ManaType},
    object_identity::ObjectDestination,
    program::{BodyTarget, ExecutableBody, Instruction},
    targets::TargetRef,
};
use std::collections::{BTreeMap, BTreeSet};
mod concession;
pub(super) mod fixture;
mod input_codec;
pub(in crate::native_game) mod projection;
mod replies;
use fixture::*;

fn shape_basis() -> GameBasis {
    // Typed identity-shape control only. No native game is initialized with this digest.
    GameBasis {
        game: GameId(u128::MAX),
        context: GameContext::new(
            "shape".into(),
            "shape".into(),
            "shape".into(),
            "shape".into(),
            "shape".into(),
            CreatureVocabularyId::try_from("0".repeat(64)).unwrap(),
            NameRules::literal_names().id(),
        )
        .unwrap(),
        epoch: u64::MAX,
        revision: u64::MAX,
    }
}
#[test]
fn binding_preserves_all_native_bits_and_rejects_invalid_roster_identity() {
    let basis = shape_basis();
    let seat = NativeSeatBinding {
        seat_id: viewer_a(),
        native_seat: A,
    };
    let value = NativeBinding::new(context_id(), basis.clone(), vec![seat], viewer_a()).unwrap();
    assert_eq!(value.public_basis().game_id, Uuid::from_u128(u128::MAX));
    assert_eq!(value.public_basis().epoch.get(), u64::MAX);
    assert_eq!(value.public_basis().revision.get(), u64::MAX);
    for (ctx, roster, viewer) in [
        (Uuid::nil(), vec![seat], viewer_a()),
        (context_id(), vec![], viewer_a()),
        (context_id(), vec![seat], Uuid::nil()),
        (context_id(), vec![seat], viewer_b()),
        (context_id(), vec![seat, seat], viewer_a()),
        (
            context_id(),
            vec![
                seat,
                NativeSeatBinding {
                    seat_id: viewer_b(),
                    native_seat: A,
                },
            ],
            viewer_a(),
        ),
        (
            context_id(),
            vec![NativeSeatBinding {
                seat_id: Uuid::nil(),
                native_seat: A,
            }],
            viewer_a(),
        ),
    ] {
        assert!(matches!(
            NativeBinding::new(ctx, basis.clone(), roster, viewer),
            Err(NativeBridgeError::InvalidBinding)
        ));
    }
    let mut nil = basis;
    nil.game = GameId(0);
    assert!(matches!(
        NativeBinding::new(context_id(), nil, vec![seat], viewer_a()),
        Err(NativeBridgeError::InvalidBinding)
    ));
}
fn instant(target: BodyTarget, instructions: Vec<Instruction>) -> Program {
    Program::instant(ManaCost::free(), ExecutableBody::new(target, instructions))
}
fn action_state() -> GameState {
    main_state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreatureOrPlayer,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
        card(
            3,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::land(
                BTreeSet::from([LandType::Island]),
                vec![ManaAbility {
                    id: ManaAbilityId(0),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
                }],
            ),
        ),
        card(
            4,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(7),
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::None,
                    body: ExecutableBody::new(
                        BodyTarget::SingleCreatureOrPlayer,
                        vec![Instruction::Damage { amount: 1.into() }],
                    ),
                }],
            ),
        ),
        card(
            5,
            A,
            ObjectDestination::Hand,
            Program::land(BTreeSet::from([LandType::Forest]), vec![]),
        ),
    ])
}
#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn native_actions_preserve_input_source_ability_payment_and_foreign_scope_boundaries() {
    let state = action_state();
    let before = state.clone();
    let mut view = scope(&state, viewer_a());
    let spell = view
        .issue(NativeReference::Object(object(&state, 1)))
        .unwrap();
    let targeted = view
        .issue(NativeReference::Object(object(&state, 2)))
        .unwrap();
    let land = view
        .issue(NativeReference::Object(object(&state, 3)))
        .unwrap();
    let artifact = view
        .issue(NativeReference::Object(object(&state, 4)))
        .unwrap();
    let hand_land = view
        .issue(NativeReference::Object(object(&state, 5)))
        .unwrap();
    let printed = view
        .issue(NativeReference::ManaAbility {
            source: object(&state, 3),
            ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
        })
        .unwrap();
    let intrinsic = view
        .issue(NativeReference::ManaAbility {
            source: object(&state, 3),
            ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
        })
        .unwrap();
    let ability = view
        .issue(NativeReference::ActivatedAbility {
            source: object(&state, 4),
            ability: ActivatedAbilityId(7),
        })
        .unwrap();
    assert!(printed != intrinsic);
    assert!(
        spell
            == view
                .issue(NativeReference::Object(object(&state, 1)))
                .unwrap()
    );
    let target = PlayerTarget::Seat {
        seat_id: viewer_b(),
    };
    let zero = PlayerPayment {
        from_lots: Default::default(),
        exile_cast_type: None,
        alternative_cost_source: None,
        graveyard_permission: None,
        use_granted_flashback: false,
        restricted_mana: Vec::new(),
        spend: vec![],
    };
    let actions = vec![
        (
            PlayerAction::CastUntargeted {
                source: spell.clone(),
                payment: None,
            },
            GameCommand::CastUntargeted {
                spell: object(&state, 1),
                payment: None,
            },
        ),
        (
            PlayerAction::CastUntargeted {
                source: spell.clone(),
                payment: Some(zero.clone()),
            },
            GameCommand::CastUntargeted {
                spell: object(&state, 1),
                payment: Some(spellwake_engine::mana::ManaPayment::default()),
            },
        ),
        (
            PlayerAction::CastTargeted {
                source: targeted.clone(),
                target: target.clone(),
                payment: None,
            },
            GameCommand::Cast {
                spell: object(&state, 2),
                target: TargetRef::Player(B),
            },
        ),
        (
            PlayerAction::CastTargeted {
                source: targeted,
                target: target.clone(),
                payment: Some(zero.clone()),
            },
            GameCommand::CastPaid {
                spell: object(&state, 2),
                target: TargetRef::Player(B),
                payment: spellwake_engine::mana::ManaPayment::default(),
            },
        ),
        (
            PlayerAction::ActivateAbility {
                source: artifact.clone(),
                ability: ability.clone(),
                target,
                payment: Some(zero),
            },
            GameCommand::ActivateAbility {
                source: object(&state, 4),
                ability: ActivatedAbilityId(7),
                target: TargetRef::Player(B),
                payment: Some(spellwake_engine::mana::ManaPayment::default()),
            },
        ),
        (
            PlayerAction::ActivateMana {
                source: land.clone(),
                ability: printed,
            },
            GameCommand::ActivateMana {
                source: object(&state, 3),
                ability: ManaAbilityRef::Printed(ManaAbilityId(0)),
            },
        ),
        (
            PlayerAction::ActivateMana {
                source: land,
                ability: intrinsic.clone(),
            },
            GameCommand::ActivateMana {
                source: object(&state, 3),
                ability: ManaAbilityRef::BasicLand(BasicLandType::Island),
            },
        ),
        (
            PlayerAction::PlayLand { source: hand_land },
            GameCommand::PlayLand {
                land: object(&state, 5),
            },
        ),
        (PlayerAction::Pass {}, GameCommand::Pass),
    ];
    for (action, expected) in actions {
        let original = command(&view, action);
        let proposed = view.propose(&original).unwrap();
        input_codec::verify(&view, &proposed);
        assert!(proposed.original() == &original);
        assert_eq!(proposed.viewer_seat_id(), viewer_a());
        assert_eq!(proposed.native_input().command, expected);
        let direct = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: A,
                command: expected,
            })
            .unwrap();
        assert_eq!(proposed.candidate(), &direct);
        assert_eq!(state, before);
    }
    // Unknown/foreign handles, wrong reference kinds and source-bound abilities fail before advance.
    let alien = PlayerHandle::try_from("opaque-unknown".to_owned()).unwrap();
    for action in [
        PlayerAction::PlayLand { source: alien },
        PlayerAction::PlayLand {
            source: ability.clone(),
        },
        PlayerAction::ActivateAbility {
            source: spell.clone(),
            ability,
            target: PlayerTarget::Seat {
                seat_id: viewer_b(),
            },
            payment: None,
        },
        PlayerAction::ActivateMana {
            source: artifact,
            ability: intrinsic,
        },
    ] {
        assert!(view.propose(&command(&view, action)).is_err());
        assert_eq!(state, before);
    }
    let mut foreign = scope(&state, viewer_b());
    let token = foreign
        .issue(NativeReference::Object(object(&state, 5)))
        .unwrap();
    assert!(
        view.propose(&command(&view, PlayerAction::PlayLand { source: token }))
            .is_err()
    );
    let mut stale = command(&view, PlayerAction::Pass {});
    stale.basis.revision = 1.into();
    assert!(matches!(
        view.propose(&stale),
        Err(NativeBridgeError::StaleBasis)
    ));
    let reordered = PlayerPayment {
        from_lots: Default::default(),
        exile_cast_type: None,
        alternative_cost_source: None,
        graveyard_permission: None,
        use_granted_flashback: false,
        restricted_mana: Vec::new(),
        spend: vec![
            ManaSpend {
                color: ManaColor::Blue,
                amount: RulesCount::try_from("18446744073709551617".to_owned()).unwrap(),
            },
            ManaSpend {
                color: ManaColor::White,
                amount: RulesCount::try_from("0".to_owned()).unwrap(),
            },
        ],
    };
    let exact = view.translate_action(&PlayerAction::CastUntargeted {
        source: spell,
        payment: Some(reordered),
    });
    let GameCommand::CastUntargeted {
        payment: Some(exact),
        ..
    } = exact.unwrap()
    else {
        panic!("expected explicit native allocation")
    };
    assert_eq!(exact.spend[0].mana_type, ManaType::Blue);
    assert_eq!(
        exact.spend[0].amount,
        crate::player_wire::RulesInteger::try_from("18446744073709551617".to_owned()).unwrap()
    );
    assert_eq!(exact.spend[1].mana_type, ManaType::White);
    assert_eq!(exact.spend[1].amount, 0.into());
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn native_payment_time_mana_color_restores_parent_and_rejects_old_handles() {
    use crate::player_wire::PlayerChoice;
    use spellwake_engine::{
        game::{DecisionReply, RuleFrame},
        program::ResolvingPlayer,
    };
    let source = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::None,
                vec![
                    Instruction::OfferManaPayment {
                        payer: ResolvingPlayer::Controller,
                        cost: ManaCost::one(ManaType::Blue),
                        paid_destination: 1,
                        declined_destination: 2,
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    },
                ],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::land(
                BTreeSet::new(),
                vec![
                    ManaAbility {
                        id: ManaAbilityId(5),
                        output: ManaOutput::ChooseColor {
                            amount: 1.into(),
                            colors: BTreeSet::from([ManaType::Blue, ManaType::White]),
                        },
                    },
                    ManaAbility {
                        id: ManaAbilityId(6),
                        output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Blue, 1.into())])),
                    },
                ],
            ),
        ),
    ]);
    let cast = bridged(&source, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let passed = bridged(&cast, viewer_a(), |_| PlayerAction::Pass {});
    let paying = bridged(&passed, viewer_b(), |_| PlayerAction::Pass {});
    assert!(matches!(
        paying.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    let before = paying.clone();
    let declined = bridged(&paying, viewer_a(), |view| PlayerAction::Choose {
        decision: view
            .issue(NativeReference::Decision(
                paying.pending_decision().unwrap().id(),
            ))
            .unwrap(),
        response: PlayerChoice::Decline {},
    });
    assert!(declined.pending_decision().is_none());
    assert_eq!(declined.execution().life_total(A), Some(&20.into()));
    let fixed = bridged(&paying, viewer_a(), |view| PlayerAction::ActivateMana {
        source: view
            .issue(NativeReference::Object(object(&paying, 2)))
            .unwrap(),
        ability: view
            .issue(NativeReference::ManaAbility {
                source: object(&paying, 2),
                ability: ManaAbilityRef::Printed(ManaAbilityId(6)),
            })
            .unwrap(),
    });
    assert!(matches!(
        fixed.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    let paid_fixed = bridged(&fixed, viewer_a(), |view| PlayerAction::Choose {
        decision: view
            .issue(NativeReference::Decision(
                fixed.pending_decision().unwrap().id(),
            ))
            .unwrap(),
        response: PlayerChoice::Pay {
            payment: PlayerPayment {
                from_lots: Default::default(),
                exile_cast_type: None,
                alternative_cost_source: None,
                graveyard_permission: None,
                use_granted_flashback: false,
                restricted_mana: Vec::new(),
                spend: vec![ManaSpend {
                    color: ManaColor::Blue,
                    amount: RulesCount::try_from("1".to_owned()).unwrap(),
                }],
            },
        },
    });
    assert_eq!(paid_fixed.execution().life_total(A), Some(&23.into()));

    let mut parent = scope(&paying, viewer_a());
    let parent_id = paying.pending_decision().unwrap().id();
    let parent_handle = parent.issue(NativeReference::Decision(parent_id)).unwrap();
    let allocation = PlayerPayment {
        from_lots: Default::default(),
        exile_cast_type: None,
        alternative_cost_source: None,
        graveyard_permission: None,
        use_granted_flashback: false,
        restricted_mana: Vec::new(),
        spend: vec![ManaSpend {
            color: ManaColor::Blue,
            amount: RulesCount::try_from("1".to_owned()).unwrap(),
        }],
    };
    let parent_command = command(
        &parent,
        PlayerAction::Choose {
            decision: parent_handle.clone(),
            response: PlayerChoice::Pay {
                payment: allocation.clone(),
            },
        },
    );
    assert!(matches!(
        parent.propose(&parent_command),
        Err(NativeBridgeError::Native(_))
    ));
    assert_eq!(paying, before);
    let land = parent
        .issue(NativeReference::Object(object(&paying, 2)))
        .unwrap();
    let ability = parent
        .issue(NativeReference::ManaAbility {
            source: object(&paying, 2),
            ability: ManaAbilityRef::Printed(ManaAbilityId(5)),
        })
        .unwrap();
    let activation = command(
        &parent,
        PlayerAction::ActivateMana {
            source: land,
            ability,
        },
    );
    let color = parent
        .propose(&activation)
        .unwrap()
        .into_parts()
        .3
        .into_state();
    assert!(matches!(
        color.pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    let mut child = scope(&color, viewer_a());
    assert!(matches!(
        child.propose(&parent_command),
        Err(NativeBridgeError::StaleBasis)
    ));
    let mut rebased_parent = parent_command.clone();
    rebased_parent.basis = child.basis().clone();
    assert!(matches!(
        child.propose(&rebased_parent),
        Err(NativeBridgeError::InvalidReference)
    ));
    let count = child.references.len();
    assert!(
        child
            .issue(NativeReference::ManaColor(ManaType::Red))
            .is_err()
    );
    assert_eq!(child.references.len(), count);
    let color_handle = child
        .issue(NativeReference::Decision(
            color.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let blue = child
        .issue(NativeReference::ManaColor(ManaType::Blue))
        .unwrap();
    let child_command = command(
        &child,
        PlayerAction::Choose {
            decision: color_handle,
            response: PlayerChoice::Option { option: blue },
        },
    );
    projection::verify_view(&mut child, Some(&child_command));
    let resolved = child.propose(&child_command).unwrap();
    input_codec::verify(&child, &resolved);
    assert!(matches!(
        resolved.native_input().command,
        GameCommand::Reply {
            choice: DecisionReply::ManaColor(ManaType::Blue),
            ..
        }
    ));
    let resumed = resolved.into_parts().3.into_state();
    assert!(matches!(
        resumed.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    let mut restored = scope(&resumed, viewer_a());
    assert!(matches!(
        restored.propose(&child_command),
        Err(NativeBridgeError::StaleBasis)
    ));
    let mut rebased_child = child_command;
    rebased_child.basis = restored.basis().clone();
    assert!(matches!(
        restored.propose(&rebased_child),
        Err(NativeBridgeError::InvalidReference)
    ));
    let handle = restored
        .issue(NativeReference::Decision(
            resumed.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let paid = command(
        &restored,
        PlayerAction::Choose {
            decision: handle,
            response: PlayerChoice::Pay {
                payment: allocation,
            },
        },
    );
    let result = restored.propose(&paid).unwrap().into_parts().3.into_state();
    assert_eq!(result.execution().life_total(A), Some(&23.into()));
    assert_eq!(result.execution().mana(A, ManaType::Blue), 0.into());
    // The kernel proposals never install into their borrowed parent snapshots.
    assert_eq!(paying, before);
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn native_source_context_basis_roster_and_incarnation_cannot_be_substituted() {
    let source = action_state();
    let before = source.clone();
    let roster = vec![
        NativeSeatBinding {
            seat_id: viewer_a(),
            native_seat: A,
        },
        NativeSeatBinding {
            seat_id: viewer_b(),
            native_seat: B,
        },
    ];
    let mut mismatches = vec![];
    let mut basis = source.basis().clone();
    basis.game = GameId(91);
    mismatches.push(basis);
    let mut basis = source.basis().clone();
    basis.epoch += 1;
    mismatches.push(basis);
    let mut basis = source.basis().clone();
    basis.revision += 1;
    mismatches.push(basis);
    let mut basis = source.basis().clone();
    let c = &basis.context;
    basis.context = GameContext::new(
        "other-release".into(),
        c.rules().into(),
        c.catalogue().into(),
        c.programs().into(),
        c.profile().into(),
        c.creature_vocabulary_id(),
        c.name_rules_id(),
    )
    .unwrap();
    mismatches.push(basis);
    for basis in mismatches {
        let bound = NativeBinding::new(context_id(), basis, roster.clone(), viewer_a()).unwrap();
        assert!(matches!(
            NativeScope::new(&source, bound),
            Err(NativeBridgeError::InvalidBinding)
        ));
    }
    let mut missing = roster.clone();
    missing.pop();
    let bound =
        NativeBinding::new(context_id(), source.basis().clone(), missing, viewer_a()).unwrap();
    assert!(matches!(
        NativeScope::new(&source, bound),
        Err(NativeBridgeError::InvalidBinding)
    ));
    let mut substituted = roster;
    substituted[1].native_seat = SeatId(900);
    let bound = NativeBinding::new(
        context_id(),
        source.basis().clone(),
        substituted,
        viewer_a(),
    )
    .unwrap();
    assert!(matches!(
        NativeScope::new(&source, bound),
        Err(NativeBridgeError::InvalidBinding)
    ));
    let mut view = scope(&source, viewer_a());
    let original = command(&view, PlayerAction::Pass {});
    let mut nil_request = original.clone();
    nil_request.request_id = Uuid::nil();
    assert!(matches!(
        view.propose(&nil_request),
        Err(NativeBridgeError::InvalidIntent)
    ));
    let mut stale = vec![];
    let mut value = original.clone();
    value.basis.game_id = Uuid::new_v4();
    stale.push(value);
    let mut value = original.clone();
    value.basis.context_id = Uuid::new_v4();
    stale.push(value);
    let mut value = original.clone();
    value.basis.epoch = 1.into();
    stale.push(value);
    let mut value = original;
    value.basis.revision = 1.into();
    stale.push(value);
    for value in stale {
        assert!(matches!(
            view.propose(&value),
            Err(NativeBridgeError::StaleBasis)
        ));
    }
    let handle = view
        .issue(NativeReference::Object(object(&source, 5)))
        .unwrap();
    let metadata = NativeBinding::new(
        Uuid::new_v4(),
        source.basis().clone(),
        vec![
            NativeSeatBinding {
                seat_id: viewer_a(),
                native_seat: A,
            },
            NativeSeatBinding {
                seat_id: viewer_b(),
                native_seat: B,
            },
        ],
        viewer_a(),
    )
    .unwrap();
    let other_context = NativeScope::new(&source, metadata).unwrap();
    let intent = command(
        &other_context,
        PlayerAction::PlayLand {
            source: handle.clone(),
        },
    );
    assert!(matches!(
        other_context.propose(&intent),
        Err(NativeBridgeError::InvalidReference)
    ));
    let seed = seed(vec![card(
        5,
        A,
        ObjectDestination::Hand,
        Program::land(BTreeSet::from([LandType::Forest]), vec![]),
    )]);
    let other_game = GameState::initialize_scheduled(
        GameId(91),
        context(&seed.creature_vocabulary),
        seed,
        spellwake_engine::turn::TurnStart {
            number: 1,
            step: spellwake_engine::turn::Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let other_game_view = scope(&other_game, viewer_a());
    let intent = command(&other_game_view, PlayerAction::PlayLand { source: handle });
    assert!(matches!(
        other_game_view.propose(&intent),
        Err(NativeBridgeError::InvalidReference)
    ));
    let hand_identity = object(&source, 5);
    let land = view.issue(NativeReference::Object(hand_identity)).unwrap();
    let play = command(&view, PlayerAction::PlayLand { source: land });
    let next = view.propose(&play).unwrap().into_parts().3.into_state();
    assert_ne!(object(&next, 5), hand_identity);
    let mut next_view = scope(&next, viewer_a());
    assert!(
        next_view
            .issue(NativeReference::Object(hand_identity))
            .is_err()
    );
    let mut rebased = play;
    rebased.basis = next_view.basis().clone();
    assert!(matches!(
        next_view.propose(&rebased),
        Err(NativeBridgeError::InvalidReference)
    ));
    let targeted = view
        .issue(NativeReference::Object(object(&source, 2)))
        .unwrap();
    let wrong_seat = command(
        &view,
        PlayerAction::CastTargeted {
            source: targeted,
            target: PlayerTarget::Seat {
                seat_id: Uuid::new_v4(),
            },
            payment: None,
        },
    );
    assert!(matches!(
        view.propose(&wrong_seat),
        Err(NativeBridgeError::InvalidReference)
    ));
    // Error output never embeds the original hidden native rejection diagnostics.
    let out_of_turn = scope(&source, viewer_b());
    let error = match out_of_turn.propose(&command(&out_of_turn, PlayerAction::Pass {})) {
        Err(error) => error,
        Ok(_) => panic!("out-of-turn accepted"),
    };
    assert!(error.native_error().is_some());
    assert!(error.observation_error().is_none());
    assert!(!format!("{error:?}").contains("SeatId"));
    assert_eq!(source, before);
}
