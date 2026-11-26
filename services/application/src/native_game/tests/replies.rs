//! Actual kernel decision frames, with exact translated input and atomic source checks.
use super::*;
use crate::player_wire::PlayerChoice;
use spellwake_engine::{
    effects::OptionalEffectChoice,
    game::{DecisionReply, RuleFrame},
    program::{AffectedPlayers, CardType, CreatureTypeSlot, PermanentTypeFilter, ResolvingPlayer},
    token::{TokenCreation, TokenDefinition, TokenName},
};

fn resolve(source: &GameState) -> GameState {
    let passed = bridged(source, viewer_a(), |_| PlayerAction::Pass {});
    bridged(&passed, viewer_b(), |_| PlayerAction::Pass {})
}
fn start(
    instructions: Vec<Instruction>,
    others: Vec<spellwake_engine::effects::SeedCard>,
) -> GameState {
    let mut cards = vec![card(
        1,
        A,
        ObjectDestination::Hand,
        instant(BodyTarget::None, instructions),
    )];
    cards.extend(others);
    let source = state(cards);
    let cast = bridged(&source, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    resolve(&cast)
}
fn rejected_proposal(
    view: &NativeScope<'_>,
    intent: &PlayerCommand,
) -> Result<PreparedNativeInput, NativeBridgeError> {
    let source = view.source.clone();
    let size = view.references.len();
    let result = view.propose(intent);
    assert!(result.is_err());
    assert_eq!(view.source, &source);
    assert_eq!(view.references.len(), size);
    result
}
fn reply(
    source: &GameState,
    response: impl FnOnce(&mut NativeScope<'_>) -> PlayerChoice,
    expected: DecisionReply,
) -> GameState {
    let before = source.clone();
    let decision = source.pending_decision().expect("actual native decision");
    let viewer = match decision.chooser() {
        A => viewer_a(),
        B => viewer_b(),
        other => panic!("unexpected chooser {other:?}"),
    };
    let mut view = scope(source, viewer);
    let handle = view
        .issue(NativeReference::Decision(decision.id()))
        .unwrap();
    for response in [
        PlayerChoice::Number { value: 1.into() },
        PlayerChoice::Distribution {
            allocations: vec![],
        },
    ] {
        let unsupported = command(
            &view,
            PlayerAction::Choose {
                decision: handle.clone(),
                response,
            },
        );
        assert!(matches!(
            rejected_proposal(&view, &unsupported),
            Err(NativeBridgeError::Unsupported)
        ));
        assert_eq!(source, &before);
    }
    let response = response(&mut view);
    let original = command(
        &view,
        PlayerAction::Choose {
            decision: handle,
            response,
        },
    );
    super::projection::verify_view(&mut view, Some(&original));
    let prepared = view.propose(&original).unwrap_or_else(|error| {
        panic!(
            "native fixture rejection: {:?}; expected {expected:?}",
            error.native_error()
        )
    });
    let expected = GameInput {
        basis: source.basis().clone(),
        actor: decision.chooser(),
        command: GameCommand::Reply {
            decision: decision.id(),
            choice: expected,
        },
    };
    super::input_codec::verify(&view, &prepared);
    assert_eq!(prepared.native_input(), &expected);
    assert!(prepared.original() == &original);
    assert_eq!(prepared.candidate(), &source.advance(expected).unwrap());
    assert_eq!(source, &before);
    let next = prepared.into_parts().3.into_state();
    let mut fresh = scope(&next, viewer);
    for (_, reference) in &view.references {
        if matches!(
            reference,
            NativeReference::Legend(_)
                | NativeReference::Trigger(_)
                | NativeReference::TokenMember(_)
                | NativeReference::DamageApplication(_)
                | NativeReference::DamageAttempt(_)
        ) {
            let size = fresh.references.len();
            assert!(fresh.issue(*reference).is_err());
            assert_eq!(fresh.references.len(), size);
        }
    }
    let mut old = original;
    old.basis = fresh.basis().clone();
    assert!(matches!(
        rejected_proposal(&fresh, &old),
        Err(NativeBridgeError::InvalidReference)
    ));
    next
}
fn objects(
    view: &mut NativeScope<'_>,
    ids: &[spellwake_engine::object_identity::ObjectId],
) -> Vec<PlayerHandle> {
    ids.iter()
        .map(|id| view.issue(NativeReference::Object(*id)).unwrap())
        .collect()
}
fn scheduled(
    cards: Vec<spellwake_engine::effects::SeedCard>,
    step: spellwake_engine::turn::Step,
) -> GameState {
    let seed = seed(cards);
    GameState::initialize_scheduled(
        GameId(90),
        context(&seed.creature_vocabulary),
        seed,
        spellwake_engine::turn::TurnStart { number: 3, step },
    )
    .unwrap()
    .into_state()
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn optional_and_creature_type_use_actual_chooser_options_and_exclusions() {
    let source = start(
        vec![
            Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            },
        ],
        vec![],
    );
    assert!(matches!(
        source.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    for (option, total) in [
        (OptionalEffectChoice::Take, 23),
        (OptionalEffectChoice::Decline, 20),
    ] {
        let result = reply(
            &source,
            |view| PlayerChoice::Option {
                option: view.issue(NativeReference::OptionalEffect(option)).unwrap(),
            },
            DecisionReply::OptionalEffect(option),
        );
        assert_eq!(result.execution().life_total(A), Some(&total.into()));
    }
    let vocab = vocabulary();
    let excluded = vocab.resolve_label("Human").unwrap();
    let allowed = vocab.resolve_label("Elf").unwrap();
    let foreign_vocabulary = spellwake_contracts::creature_types::CreatureTypeVocabulary::new(
        vocab.rules_revision().to_owned(),
        *vocab.rules_sha256(),
        vec!["Elf".to_owned()],
    )
    .unwrap();
    let foreign_member = foreign_vocabulary.resolve_label("Elf").unwrap();

    let source = start(
        vec![
            Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::from([excluded]),
            },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(2.into()),
            },
        ],
        vec![],
    );
    assert!(matches!(
        source.pending_decision().unwrap().frame(),
        RuleFrame::CreatureType(_)
    ));
    let mut view = scope(&source, viewer_a());
    let before = view.references.len();
    assert!(view.issue(NativeReference::CreatureType(excluded)).is_err());
    assert!(
        view.issue(NativeReference::CreatureType(foreign_member))
            .is_err()
    );
    assert_eq!(view.references.len(), before);
    let mut wrong_viewer = scope(&source, viewer_b());
    assert!(
        wrong_viewer
            .issue(NativeReference::Decision(
                source.pending_decision().unwrap().id()
            ))
            .is_err()
    );
    let result = reply(
        &source,
        |view| PlayerChoice::Option {
            option: view.issue(NativeReference::CreatureType(allowed)).unwrap(),
        },
        DecisionReply::CreatureType(allowed),
    );
    assert_eq!(result.execution().life_total(A), Some(&22.into()));
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn sacrifice_selection_and_resolution_graveyard_order_keep_native_identity_and_order() {
    let source = start(
        vec![Instruction::Sacrifice {
            players: AffectedPlayers::One(ResolvingPlayer::Controller),
            amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
            filter: PermanentTypeFilter {
                any_types: Default::default(),
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::new(),
                object_filter: None,
            },
        }],
        (2..=4)
            .map(|id| {
                card(
                    id,
                    A,
                    ObjectDestination::Battlefield { controller: A },
                    Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
                )
            })
            .collect(),
    );
    let RuleFrame::ResolutionSelect(frame) = source.pending_decision().unwrap().frame() else {
        panic!("selection frame")
    };
    assert_eq!(frame.required(), 2);
    let selected = vec![frame.domain()[2], frame.domain()[0]];
    let mut invalid = scope(&source, viewer_a());
    let handle = invalid
        .issue(NativeReference::Decision(
            source.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let too_few = objects(&mut invalid, &selected[..1]);
    let rejected = command(
        &invalid,
        PlayerAction::Choose {
            decision: handle,
            response: PlayerChoice::Objects { objects: too_few },
        },
    );
    assert!(matches!(
        rejected_proposal(&invalid, &rejected),
        Err(NativeBridgeError::Native(_))
    ));
    let ordered = reply(
        &source,
        |view| PlayerChoice::Objects {
            objects: objects(view, &selected),
        },
        DecisionReply::SelectResolution(selected.clone()),
    );
    let RuleFrame::ResolutionGraveyardOrder(frame) = ordered.pending_decision().unwrap().frame()
    else {
        panic!("resolution graveyard frame")
    };
    let order = frame.domain().iter().rev().copied().collect::<Vec<_>>();
    let result = reply(
        &ordered,
        |view| PlayerChoice::Ordering {
            ordered: objects(view, &order),
        },
        DecisionReply::OrderResolutionGraveyard(order.clone()),
    );
    assert!(result.pending_decision().is_none());
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn simultaneous_token_timestamp_members_retain_reverse_order_and_reject_objects() {
    let definition = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &vocabulary(),
    )
    .unwrap();
    let source = start(
        vec![Instruction::CreateTokens {
            creations: vec![
                TokenCreation::one(definition.clone()),
                TokenCreation::one(definition),
            ],
        }],
        vec![],
    );
    let RuleFrame::TimestampOrder(frame) = source.pending_decision().unwrap().frame() else {
        panic!("timestamp frame")
    };
    let members = frame
        .members()
        .iter()
        .rev()
        .map(|entry| entry.member)
        .collect::<Vec<_>>();
    assert_eq!(members.len(), 2);
    let mut wrong = scope(&source, viewer_a());
    let d = wrong
        .issue(NativeReference::Decision(
            source.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let object = wrong
        .issue(NativeReference::Object(object(&source, 1)))
        .unwrap();
    let request = command(
        &wrong,
        PlayerAction::Choose {
            decision: d,
            response: PlayerChoice::Ordering {
                ordered: vec![object],
            },
        },
    );
    assert!(matches!(
        rejected_proposal(&wrong, &request),
        Err(NativeBridgeError::WrongReferenceKind)
    ));
    let result = reply(
        &source,
        |view| PlayerChoice::Ordering {
            ordered: members
                .iter()
                .map(|member| view.issue(NativeReference::TokenMember(*member)).unwrap())
                .collect(),
        },
        DecisionReply::TimestampOrder(members.clone()),
    );
    assert!(result.pending_decision().is_none());
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn legend_keep_and_check_graveyard_order_use_frozen_conflict_and_owner_domain() {
    use spellwake_contracts::names::{EnglishCardName, ObjectNames};
    use spellwake_engine::{checks::CheckDecision, program::Supertype};
    let cards = (1..=3)
        .map(|id| {
            let mut program = Program::artifact(None, vec![]);
            program.supertypes.insert(Supertype::Legendary);
            program.names =
                ObjectNames::single(EnglishCardName::new("Bridge legend fixture".into()).unwrap());
            card(
                id,
                A,
                ObjectDestination::Battlefield { controller: A },
                program,
            )
        })
        .collect();
    let source = main_state(cards);
    let RuleFrame::Checks(CheckDecision::Legend(frame)) =
        source.pending_decision().unwrap().frame()
    else {
        panic!("legend frame")
    };
    let conflict = frame.conflicts()[0].id();
    let keep = frame.conflicts()[0].domain()[1];
    let ordered = reply(
        &source,
        |view| PlayerChoice::Legend {
            conflict: view.issue(NativeReference::Legend(conflict)).unwrap(),
            keep: view.issue(NativeReference::Object(keep)).unwrap(),
        },
        DecisionReply::KeepLegend { conflict, keep },
    );
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) =
        ordered.pending_decision().unwrap().frame()
    else {
        panic!("check graveyard frame")
    };
    let order = frame.domain().iter().rev().copied().collect::<Vec<_>>();
    assert_eq!(order.len(), 2);
    let result = reply(
        &ordered,
        |view| PlayerChoice::Ordering {
            ordered: objects(view, &order),
        },
        DecisionReply::OrderGraveyard(order.clone()),
    );
    assert!(result.execution().window().cards().get(keep).is_some());
    assert!(result.pending_decision().is_none());
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn cleanup_discard_keeps_selection_order_without_an_extra_order_decision() {
    use spellwake_engine::{checks::CheckDecision, turn::Step};
    let source = scheduled(
        (1..=9)
            .map(|id| {
                card(
                    id,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(None, vec![]),
                )
            })
            .collect(),
        Step::Cleanup,
    );
    let RuleFrame::Checks(CheckDecision::CleanupDiscard(frame)) =
        source.pending_decision().unwrap().frame()
    else {
        panic!("cleanup frame")
    };
    assert_eq!(frame.required(), 2);
    let selected = vec![frame.domain()[1], frame.domain()[0]];
    let result = reply(
        &source,
        |view| PlayerChoice::Objects {
            objects: objects(view, &selected),
        },
        DecisionReply::Discard(selected.clone()),
    );
    assert!(result.pending_decision().is_none());
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn opening_mulligan_and_bottom_preserve_order_and_reject_ordering_syntax() {
    use spellwake_engine::{
        game::OpeningSetup,
        opening::{MulliganChoice, OpeningDecision},
        shuffle::EntropyTape,
    };
    let seed = seed(
        [(A, 100), (B, 200)]
            .into_iter()
            .flat_map(|(owner, base)| {
                (0..20).map(move |n| {
                    card(
                        base + n,
                        owner,
                        ObjectDestination::LibraryBottom,
                        Program::artifact(None, vec![]),
                    )
                })
            })
            .collect(),
    );
    let mut current = GameState::initialize_opening(
        GameId(90),
        context(&seed.creature_vocabulary),
        OpeningSetup {
            seed,
            entropy: EntropyTape::new(vec![0; 1000]),
        },
    )
    .unwrap()
    .into_state();
    let mut a_mulligans = 0;
    let mut two_card_bottom_seen = false;
    for _ in 0..16 {
        let Some(decision) = current.pending_decision() else {
            break;
        };
        match decision.frame() {
            RuleFrame::Opening(OpeningDecision::Declare { .. }) => {
                let choice = if decision.chooser() == A && a_mulligans < 2 {
                    a_mulligans += 1;
                    MulliganChoice::Mulligan
                } else {
                    MulliganChoice::Keep
                };
                current = reply(
                    &current,
                    |view| PlayerChoice::Option {
                        option: view.issue(NativeReference::Mulligan(choice)).unwrap(),
                    },
                    DecisionReply::Mulligan(choice),
                );
            }
            RuleFrame::Opening(OpeningDecision::Bottom { count, hand, .. }) => {
                let selected = hand.iter().take(*count).rev().copied().collect::<Vec<_>>();
                if *count == 2 {
                    two_card_bottom_seen = true;
                    let mut view = scope(&current, viewer_a());
                    let decision = view
                        .issue(NativeReference::Decision(decision.id()))
                        .unwrap();
                    let ordered = objects(&mut view, &selected);
                    let invalid = command(
                        &view,
                        PlayerAction::Choose {
                            decision,
                            response: PlayerChoice::Ordering { ordered },
                        },
                    );
                    assert!(matches!(
                        rejected_proposal(&view, &invalid),
                        Err(NativeBridgeError::Unsupported)
                    ));
                }
                current = reply(
                    &current,
                    |view| PlayerChoice::Objects {
                        objects: objects(view, &selected),
                    },
                    DecisionReply::Bottom(selected.clone()),
                );
            }
            _ => panic!("opening frame"),
        }
    }
    assert!(two_card_bottom_seen);
    assert!(current.opening().unwrap().complete());
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn copied_spell_retain_and_changed_targets_are_bound_options() {
    use spellwake_engine::effects::CopyTargetChoice;
    let source = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreatureOrPlayer,
                vec![Instruction::Damage { amount: 1.into() }],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleSpell(
                    spellwake_engine::program::SpellFilter::copyable_instruction(),
                ),
                vec![Instruction::CopySpell],
            ),
        ),
    ]);
    let cast = bridged(&source, viewer_a(), |view| PlayerAction::CastTargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        target: PlayerTarget::Seat {
            seat_id: viewer_b(),
        },
        payment: None,
    });
    let original = cast.execution().window().entries()[0].id();
    let copied = bridged(&cast, viewer_a(), |view| PlayerAction::CastTargeted {
        source: view
            .issue(NativeReference::Object(object(&cast, 2)))
            .unwrap(),
        target: PlayerTarget::Stack {
            handle: view.issue(NativeReference::Stack(original)).unwrap(),
        },
        payment: None,
    });
    let pending = resolve(&copied);
    assert!(matches!(
        pending.pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    let mut wrong = scope(&pending, viewer_a());
    let decision = wrong
        .issue(NativeReference::Decision(
            pending.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let request = command(
        &wrong,
        PlayerAction::Choose {
            decision,
            response: PlayerChoice::Target {
                target: PlayerTarget::Seat {
                    seat_id: viewer_a(),
                },
            },
        },
    );
    assert!(matches!(
        rejected_proposal(&wrong, &request),
        Err(NativeBridgeError::Unsupported)
    ));
    for choice in [
        CopyTargetChoice::Retain,
        CopyTargetChoice::Change(TargetRef::Player(A)),
    ] {
        let result = reply(
            &pending,
            |view| PlayerChoice::Option {
                option: view.issue(NativeReference::CopyTargets(choice)).unwrap(),
            },
            DecisionReply::CopyTargets(choice),
        );
        assert_eq!(result.execution().window().spell_copies().len(), 1);
    }
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn trigger_order_then_target_use_actual_instances_and_controller() {
    use spellwake_engine::program::{
        PlayerRelation, TriggerGrouping, TriggeredAbility, TriggeredAbilityId,
    };
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered = (1..=2)
        .map(|id| TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: spellwake_engine::program::TriggerSpec::spell_cast(
                PlayerRelation::Controller,
                spellwake_engine::program::SpellFilter::any(),
            ),
            grouping: TriggerGrouping::EachOccurrence,
            body: if id == 1 {
                ExecutableBody::new(
                    BodyTarget::SingleCreatureOrPlayer,
                    vec![Instruction::Damage { amount: 1.into() }],
                )
            } else {
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(1.into()),
                    }],
                )
            },
        })
        .collect();
    let source = state(vec![
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
            ObjectDestination::Battlefield { controller: A },
            watcher,
        ),
    ]);
    let cast = bridged(&source, viewer_a(), |view| PlayerAction::CastUntargeted {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let RuleFrame::Triggers(frame) = cast.pending_decision().unwrap().frame() else {
        panic!("trigger order")
    };
    let order = frame.instances().iter().rev().copied().collect::<Vec<_>>();
    let targeting = reply(
        &cast,
        |view| PlayerChoice::Ordering {
            ordered: order
                .iter()
                .map(|id| view.issue(NativeReference::Trigger(*id)).unwrap())
                .collect(),
        },
        DecisionReply::OrderTriggers(order.clone()),
    );
    assert!(matches!(
        targeting.pending_decision().unwrap().frame(),
        RuleFrame::TriggerTarget(_)
    ));
    let result = reply(
        &targeting,
        |_| PlayerChoice::Target {
            target: PlayerTarget::Seat {
                seat_id: viewer_b(),
            },
        },
        DecisionReply::TriggerTarget(TargetRef::Player(B)),
    );
    assert!(result.pending_decision().is_none());
    assert_eq!(result.execution().window().entries().len(), 3);
}

fn pass_current(source: &GameState) -> GameState {
    use spellwake_engine::{priority::PriorityPhase, stack_window::WindowPhase};
    let WindowPhase::Priority(PriorityPhase::Ready { holder }) =
        source.execution().window().phase()
    else {
        panic!("actual priority holder")
    };
    let viewer = match holder {
        A => viewer_a(),
        B => viewer_b(),
        _ => panic!("unexpected holder"),
    };
    bridged(source, viewer, |_| PlayerAction::Pass {})
}
fn count(value: i64) -> RulesCount {
    RulesCount::try_from(value.to_string()).unwrap()
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn singleton_damage_effect_order_uses_only_current_offered_application() {
    use spellwake_engine::effects::DamageDecision;
    let source = state(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::CreatePreventionShield { amount: 2.into() }],
            ),
        ),
        card(
            2,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::CreatePreventionShield { amount: 4.into() }],
            ),
        ),
        card(
            3,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::Damage { amount: 3.into() }],
            ),
        ),
        card(
            4,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 1.into(), 10.into(), BTreeSet::new()),
        ),
    ]);
    let mut current = source;
    for id in [1, 2, 3] {
        let cast = bridged(&current, viewer_a(), |view| PlayerAction::CastTargeted {
            source: view
                .issue(NativeReference::Object(object(&current, id)))
                .unwrap(),
            target: PlayerTarget::Object {
                handle: view
                    .issue(NativeReference::Object(object(&current, 4)))
                    .unwrap(),
            },
            payment: None,
        });
        current = resolve(&cast);
    }
    let RuleFrame::Damage(DamageDecision::Order(frame)) =
        current.pending_decision().unwrap().frame()
    else {
        panic!("singleton damage order")
    };
    assert_eq!(frame.applications().len(), 2);
    let application = frame.applications()[1];
    let result = reply(
        &current,
        |view| PlayerChoice::Option {
            option: view
                .issue(NativeReference::DamageApplication(application))
                .unwrap(),
        },
        DecisionReply::DamageOrder(application),
    );
    assert!(result.pending_decision().is_none());
    assert_eq!(
        result
            .execution()
            .creature(object(&result, 4))
            .unwrap()
            .unwrap()
            .damage,
        0.into()
    );
}

#[test]
#[ignore = "requires explicitly supplied source-qualified vocabulary; native adapter composition, not card qualification"]
fn combat_declarations_damage_order_and_prevention_use_actual_simultaneous_frames() {
    use crate::player_wire::{
        AttackerAssignment, BlockerAssignment, CombatAllocation, PreventionAllocation,
    };
    use spellwake_engine::{
        combat::{AttackDeclaration, BlockDeclaration, CombatDamageAssignment, CombatDecision},
        damage::{DamageAllocation, DamageRecipient},
        effects::DamageDecision,
        turn::Step,
    };
    let mut cards = vec![];
    for (id, owner, power, toughness) in
        [(1, A, 4, 10), (2, A, 1, 10), (3, B, 2, 10), (4, B, 2, 10)]
    {
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            Program::creature(None, power.into(), toughness.into(), BTreeSet::new()),
        ));
    }
    {
        let (id, amount) = (50, 3);
        cards.push(card(
            id,
            A,
            ObjectDestination::Hand,
            instant(
                BodyTarget::SingleCreature,
                vec![Instruction::CreatePreventionShield {
                    amount: amount.into(),
                }],
            ),
        ));
    }
    cards.push(card(
        99,
        A,
        ObjectDestination::Battlefield { controller: A },
        Program::artifact(None, vec![]),
    ));
    for (id, owner) in [(100, A), (200, B)] {
        cards.push(card(
            id,
            owner,
            ObjectDestination::LibraryBottom,
            Program::artifact(None, vec![]),
        ));
    }
    let mut current = scheduled(cards, Step::Untap);
    {
        let id = 50;
        let cast = bridged(&current, viewer_a(), |view| PlayerAction::CastTargeted {
            source: view
                .issue(NativeReference::Object(object(&current, id)))
                .unwrap(),
            target: PlayerTarget::Object {
                handle: view
                    .issue(NativeReference::Object(object(&current, 1)))
                    .unwrap(),
            },
            payment: None,
        });
        current = resolve(&cast);
    }
    for _ in 0..32 {
        if current.pending_decision().is_some() {
            break;
        }
        current = pass_current(&current);
    }
    assert!(matches!(
        current.pending_decision().unwrap().frame(),
        RuleFrame::Combat(CombatDecision::Attackers(_))
    ));
    let attackers = [object(&current, 1), object(&current, 2)];
    let mut wrong = scope(&current, viewer_a());
    let decision = wrong
        .issue(NativeReference::Decision(
            current.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let attacker = wrong.issue(NativeReference::Object(attackers[0])).unwrap();
    let invalid = command(
        &wrong,
        PlayerAction::Choose {
            decision,
            response: PlayerChoice::Attackers {
                assignments: vec![AttackerAssignment {
                    band: None,
                    attacker: attacker.clone(),
                    defender: PlayerTarget::Object { handle: attacker },
                }],
            },
        },
    );
    assert!(matches!(
        rejected_proposal(&wrong, &invalid),
        Err(NativeBridgeError::Unsupported)
    ));

    current = reply(
        &current,
        |view| PlayerChoice::Attackers {
            assignments: attackers
                .iter()
                .map(|id| AttackerAssignment {
                    band: None,
                    attacker: view.issue(NativeReference::Object(*id)).unwrap(),
                    defender: PlayerTarget::Seat {
                        seat_id: viewer_b(),
                    },
                })
                .collect(),
        },
        DecisionReply::DeclareAttackers(
            attackers
                .iter()
                .map(|id| AttackDeclaration {
                    attacker: *id,
                    defender: B,
                    defending_planeswalker: None,
                })
                .collect(),
        ),
    );
    current = pass_current(&pass_current(&current));
    assert!(matches!(
        current.pending_decision().unwrap().frame(),
        RuleFrame::Combat(CombatDecision::Blockers(_))
    ));
    let blockers = [object(&current, 3), object(&current, 4)];
    let mut multi = scope(&current, viewer_b());
    let decision = multi
        .issue(NativeReference::Decision(
            current.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let blocker = multi.issue(NativeReference::Object(blockers[0])).unwrap();
    let reverse = vec![
        multi.issue(NativeReference::Object(attackers[1])).unwrap(),
        multi.issue(NativeReference::Object(attackers[0])).unwrap(),
    ];
    let action = PlayerAction::Choose {
        decision,
        response: PlayerChoice::Blockers {
            assignments: vec![BlockerAssignment {
                blocker,
                attackers: reverse,
            }],
        },
    };
    let translated = multi.translate_action(&action).unwrap();
    assert!(
        matches!(translated, GameCommand::Reply { choice: DecisionReply::DeclareBlockers(ref values), .. } if values == &vec![BlockDeclaration { blocker: blockers[0], attacker: attackers[1] }, BlockDeclaration { blocker: blockers[0], attacker: attackers[0] }])
    );
    assert!(matches!(
        multi.propose(&command(&multi, action)),
        Err(NativeBridgeError::Native(_))
    ));

    current = reply(
        &current,
        |view| PlayerChoice::Blockers {
            assignments: blockers
                .iter()
                .map(|id| BlockerAssignment {
                    blocker: view.issue(NativeReference::Object(*id)).unwrap(),
                    attackers: vec![view.issue(NativeReference::Object(attackers[0])).unwrap()],
                })
                .collect(),
        },
        DecisionReply::DeclareBlockers(
            blockers
                .iter()
                .map(|id| BlockDeclaration {
                    blocker: *id,
                    attacker: attackers[0],
                })
                .collect(),
        ),
    );
    current = pass_current(&pass_current(&current));
    let RuleFrame::Combat(CombatDecision::DamageAssignments(frame)) =
        current.pending_decision().unwrap().frame()
    else {
        panic!("combat damage assignment")
    };
    assert_eq!(frame.domains().len(), 2);
    let mut wrong = scope(&current, viewer_a());
    let decision = wrong
        .issue(NativeReference::Decision(
            current.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let source_handle = wrong.issue(NativeReference::Object(attackers[0])).unwrap();
    let invalid = command(
        &wrong,
        PlayerAction::Choose {
            decision,
            response: PlayerChoice::CombatDamage {
                assignments: vec![CombatAllocation {
                    source: source_handle.clone(),
                    target: PlayerTarget::Stack {
                        handle: source_handle,
                    },
                    amount: count(4),
                }],
            },
        },
    );
    assert!(matches!(
        rejected_proposal(&wrong, &invalid),
        Err(NativeBridgeError::Unsupported)
    ));

    let mut wrong = scope(&current, viewer_a());
    let decision = wrong
        .issue(NativeReference::Decision(
            current.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let source_handle = wrong.issue(NativeReference::Object(attackers[0])).unwrap();
    let artifact = wrong
        .issue(NativeReference::Object(object(&current, 99)))
        .unwrap();
    let invalid = command(
        &wrong,
        PlayerAction::Choose {
            decision,
            response: PlayerChoice::CombatDamage {
                assignments: vec![CombatAllocation {
                    source: source_handle,
                    target: PlayerTarget::Object { handle: artifact },
                    amount: count(4),
                }],
            },
        },
    );
    assert!(matches!(
        rejected_proposal(&wrong, &invalid),
        Err(NativeBridgeError::Unsupported)
    ));
    let assignments = vec![
        CombatDamageAssignment {
            source: attackers[0],
            target: DamageRecipient::Creature(blockers[0]),
            amount: 2.into(),
        },
        CombatDamageAssignment {
            source: attackers[0],
            target: DamageRecipient::Creature(blockers[1]),
            amount: 2.into(),
        },
        CombatDamageAssignment {
            source: attackers[1],
            target: DamageRecipient::Player(B),
            amount: 1.into(),
        },
    ];
    current = reply(
        &current,
        |view| PlayerChoice::CombatDamage {
            assignments: assignments
                .iter()
                .map(|entry| CombatAllocation {
                    source: view.issue(NativeReference::Object(entry.source)).unwrap(),
                    target: match entry.target {
                        DamageRecipient::Creature(id)
                        | DamageRecipient::Planeswalker(id)
                        | DamageRecipient::Battle(id) => PlayerTarget::Object {
                            handle: view.issue(NativeReference::Object(id)).unwrap(),
                        },
                        DamageRecipient::Player(B) => PlayerTarget::Seat {
                            seat_id: viewer_b(),
                        },
                        _ => panic!("fixture target"),
                    },
                    amount: RulesCount::try_from(String::from(entry.amount.clone())).unwrap(),
                })
                .collect(),
        },
        DecisionReply::AssignCombatDamage(assignments.clone()),
    );
    let RuleFrame::Damage(DamageDecision::Allocation(frame)) =
        current.pending_decision().unwrap().frame()
    else {
        panic!("prevention allocation")
    };
    assert_eq!(frame.required(), &3.into());
    assert_eq!(frame.eligible().len(), 2);
    let allocations = vec![
        DamageAllocation {
            attempt: frame.eligible()[1].attempt(),
            amount: 2.into(),
        },
        DamageAllocation {
            attempt: frame.eligible()[0].attempt(),
            amount: 1.into(),
        },
    ];
    current = reply(
        &current,
        |view| PlayerChoice::Prevention {
            allocations: allocations
                .iter()
                .map(|entry| PreventionAllocation {
                    attempt: view
                        .issue(NativeReference::DamageAttempt(entry.attempt))
                        .unwrap(),
                    amount: if entry.amount == 2.into() {
                        count(2)
                    } else {
                        count(1)
                    },
                })
                .collect(),
        },
        DecisionReply::AllocatePrevention(allocations.clone()),
    );
    assert!(current.pending_decision().is_none());
    assert_eq!(current.execution().life_total(B), Some(&19.into()));
    assert_eq!(
        current
            .execution()
            .creature(attackers[0])
            .unwrap()
            .unwrap()
            .damage,
        1.into()
    );
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn storm_controller_selects_each_copy_target_through_player_actions() {
    use spellwake_engine::{
        effects::CopyTargetChoice,
        life::LifeAdjustment,
        program::{CastCopyKeyword, TriggeredAbility, TriggeredAbilityId},
    };
    let mut storm = instant(
        BodyTarget::SinglePlayer,
        vec![Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Lose(2.into()),
        }],
    );
    storm.triggered.push(TriggeredAbility::spell_copies(
        TriggeredAbilityId(31),
        CastCopyKeyword::Storm,
    ));
    let source = state(vec![
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
        card(2, A, ObjectDestination::Hand, storm),
    ]);
    let first = bridged(&source, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let first = resolve(&first);
    let cast = bridged(&first, viewer_a(), |v| PlayerAction::CastTargeted {
        source: v.issue(NativeReference::Object(object(&first, 2))).unwrap(),
        target: PlayerTarget::Seat {
            seat_id: viewer_b(),
        },
        payment: None,
    });
    let pending = resolve(&cast);
    assert_eq!(pending.pending_decision().unwrap().chooser(), A);
    for choice in [
        CopyTargetChoice::Retain,
        CopyTargetChoice::Change(TargetRef::Player(A)),
    ] {
        let copied = reply(
            &pending,
            |v| PlayerChoice::Option {
                option: v.issue(NativeReference::CopyTargets(choice)).unwrap(),
            },
            DecisionReply::CopyTargets(choice),
        );
        assert_eq!(copied.execution().window().spell_copies().len(), 1);
        let done = resolve(&resolve(&copied));
        let remaining = if choice == CopyTargetChoice::Retain {
            16
        } else {
            18
        };
        assert_eq!(done.execution().life_total(B), Some(&remaining.into()));
        assert_eq!(
            done.execution().cast_turn_history().unwrap().spells,
            2.into()
        );
    }
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn shockland_entry_pay_life_projects_and_translates_accept_and_decline() {
    use crate::player_wire::{DecisionKind, PlayerDomain};
    use spellwake_engine::{land::EntryTap, program::PermanentDefinition};

    let mut shock = Program::land(BTreeSet::from([LandType::Forest, LandType::Island]), vec![]);
    if let spellwake_engine::program::ProgramKind::Permanent(PermanentDefinition {
        entry_tap,
        ..
    }) = &mut shock.kind
    {
        *entry_tap = Some(EntryTap::UnlessPayLife(2));
    }
    let source = main_state(vec![card(1, A, ObjectDestination::Hand, shock)]);
    let played = bridged(&source, viewer_a(), |view| PlayerAction::PlayLand {
        source: view
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
    });
    assert!(matches!(
        played.pending_decision().map(|d| d.frame()),
        Some(RuleFrame::EntryChoice(_))
    ));
    let mut view = scope(&played, viewer_a());
    let snap = super::projection::project_test(&mut view).unwrap();
    assert_eq!(snap.decisions.len(), 1);
    assert_eq!(snap.decisions[0].kind, DecisionKind::OptionalEffect);
    let PlayerDomain::Options { options, .. } = &snap.decisions[0].domain else {
        panic!("expected Options domain for Shockland entry");
    };
    assert_eq!(options.len(), 2);
    assert!(options[0].label.contains("Pay 2 life"));

    // Accept: enters untapped and loses 2 life.
    let paid = reply(
        &played,
        |v| PlayerChoice::Option {
            option: v.issue(NativeReference::EntryPayLife(true)).unwrap(),
        },
        DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::PayLife(true)),
    );
    assert_eq!(paid.execution().life_total(A), Some(&18.into()));
    let land_id = object(&paid, 1);
    assert_eq!(
        paid.execution()
            .window()
            .cards()
            .get(land_id)
            .unwrap()
            .tapped(),
        Some(false)
    );

    // Decline: enters tapped and keeps 20 life.
    let declined = reply(
        &played,
        |v| PlayerChoice::Option {
            option: v.issue(NativeReference::EntryPayLife(false)).unwrap(),
        },
        DecisionReply::EntryChoice(spellwake_engine::land::EntryChoice::PayLife(false)),
    );
    assert_eq!(declined.execution().life_total(A), Some(&20.into()));
    let land_id = object(&declined, 1);
    assert_eq!(
        declined
            .execution()
            .window()
            .cards()
            .get(land_id)
            .unwrap()
            .tapped(),
        Some(true)
    );
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn option_index_choice_bundle_projects_human_labels_and_translates_round_trip() {
    use crate::player_wire::{DecisionKind, PlayerDomain};
    use spellwake_engine::program::EffectManaYield;

    let mut ww = BTreeMap::new();
    ww.insert(ManaType::White, 2.into());
    let mut bb = BTreeMap::new();
    bb.insert(ManaType::Black, 2.into());
    let spell = instant(
        BodyTarget::None,
        vec![Instruction::AddMana(EffectManaYield::ChoiceBundle {
            options: vec![ww, bb],
        })],
    );
    let source = main_state(vec![card(1, A, ObjectDestination::Hand, spell)]);
    let cast = bridged(&source, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let pending = resolve(&cast);
    let mut view = scope(&pending, viewer_a());
    let snap = super::projection::project_test(&mut view).unwrap();
    assert_eq!(snap.decisions.len(), 1);
    assert_eq!(snap.decisions[0].kind, DecisionKind::Modes);
    let PlayerDomain::Options { options, .. } = &snap.decisions[0].domain else {
        panic!("expected Options domain for OptionIndex");
    };
    assert_eq!(options.len(), 2);
    assert_eq!(options[0].label, "Add {W}{W}");
    assert_eq!(options[1].label, "Add {B}{B}");

    let done = reply(
        &pending,
        |v| PlayerChoice::Option {
            option: v.issue(NativeReference::OptionIndex(1)).unwrap(),
        },
        DecisionReply::OptionIndex(1),
    );
    assert_eq!(
        done.execution().unrestricted_mana(A, ManaType::Black),
        2.into()
    );
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn energy_counter_myriad_keyword_and_extended_counter_kinds_project_cleanly() {
    use crate::player_wire::{PlayerCounterKind, PlayerKeyword};
    use spellwake_engine::{
        effects::CounterKind,
        program::{
            AttackTriggerMode, TriggerEvent, TriggerGrouping, TriggerSpec, TriggeredAbility,
            TriggeredAbilityId,
        },
    };

    // Extended PlayerCounterKind covers modern counter kinds without collapsing or failing.
    for kind in [
        CounterKind::Stun,
        CounterKind::Shield,
        CounterKind::Oil,
        CounterKind::Finality,
        CounterKind::Lore,
        CounterKind::Defense,
        CounterKind::Flying,
        CounterKind::Indestructible,
    ] {
        let wire = PlayerCounterKind::try_from(kind).expect("extended wire counter kind");
        let roundtrip: CounterKind = wire.into();
        assert_eq!(roundtrip, kind);
    }

    // Energy counters appear on PlayerSeatView.counters and Myriad projects as Myriad (not Annihilator).
    let mut myriad_creature = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    myriad_creature.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: TriggerSpec::new(TriggerEvent::Attacks {
            subject: spellwake_engine::program::ObjectFilter::self_object(),
            mode: AttackTriggerMode::Individual {
                capture_defender: true,
            },
        }),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![Instruction::Myriad]),
    });
    let energy_spell = instant(
        BodyTarget::None,
        vec![Instruction::AddPlayerCounter {
            players: AffectedPlayers::One(ResolvingPlayer::Controller),
            kind: CounterKind::Energy,
            amount: spellwake_engine::program::ValueExpr::Literal(3.into()),
        }],
    );
    let source = main_state(vec![
        card(1, A, ObjectDestination::Hand, energy_spell),
        card(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            myriad_creature,
        ),
    ]);
    let cast = bridged(&source, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let done = resolve(&cast);
    let mut view = scope(&done, viewer_a());
    let snap = super::projection::project_test(&mut view).unwrap();
    let seat_a = snap
        .players
        .iter()
        .find(|s| s.seat_id == viewer_a())
        .unwrap();
    assert!(
        seat_a
            .counters
            .iter()
            .any(|c| c.name == "energy" && *c.count.as_rules_integer() == 3.into()),
        "expected energy counter of 3 in seat view"
    );
    let bf_obj = snap
        .visible_objects
        .iter()
        .find(|o| o.public_properties.is_some())
        .unwrap();
    let props = bf_obj.public_properties.as_ref().unwrap();
    assert!(
        props.keywords.contains(&PlayerKeyword::Myriad),
        "expected Myriad keyword in public properties"
    );
    assert!(
        !props.keywords.contains(&PlayerKeyword::Annihilator),
        "Myriad must not project as Annihilator"
    );
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn predicate_restricted_mana_projects_enforces_atomically_and_round_trips_wire() {
    use crate::player_wire::{
        ManaColor, ManaSpend, PlayerManaRestriction, PlayerPayment, PlayerRestrictedManaSpend,
        RulesCount,
    };
    use spellwake_engine::{
        mana::{ManaRestriction, ManaSpendPredicate, SpellManaClause},
        object_identity::ObjectZone,
        program::{EffectManaYield, Supertype},
    };

    let rc = |n: &str| RulesCount::try_from(n.to_owned()).unwrap();

    // 1. Jegantha-style disallow_generic predicate + Legendary-only predicate.
    let mut no_generic_pred = ManaSpendPredicate {
        allow_any_spell: true,
        disallow_generic: true,
        ..Default::default()
    };
    no_generic_pred.canonicalize();
    let no_generic_rule = ManaRestriction::Predicate(no_generic_pred);

    let mut legendary_pred = ManaSpendPredicate {
        spell_clauses: vec![SpellManaClause {
            supertype: Some(Supertype::Legendary),
            ..Default::default()
        }],
        makes_spell_uncounterable: true,
        ..Default::default()
    };
    legendary_pred.canonicalize();
    let legendary_rule = ManaRestriction::Predicate(legendary_pred);

    let producer = instant(
        BodyTarget::None,
        vec![
            Instruction::AddMana(EffectManaYield::RestrictedFixed {
                bundle: [(ManaType::Red, 2.into())].into(),
                restriction: no_generic_rule,
            }),
            Instruction::AddMana(EffectManaYield::RestrictedFixed {
                bundle: [(ManaType::Green, 1.into())].into(),
                restriction: legendary_rule,
            }),
        ],
    );
    // Costs {1}{R}: cannot be paid with 2 no-generic {R} because generic {1} cannot use disallow_generic mana.
    let mut generic_red_cost = ManaCost::free();
    generic_red_cost.generic = 1.into();
    generic_red_cost.required.insert(ManaType::Red, 1.into());
    let mut nonlegendary_creature =
        Program::creature(Some(generic_red_cost), 2.into(), 2.into(), BTreeSet::new());
    nonlegendary_creature.card_types.insert(CardType::Creature);

    // Legendary creature costing {G}: payable by legendary_rule {G}.
    let mut green_cost = ManaCost::free();
    green_cost.required.insert(ManaType::Green, 1.into());
    let mut legendary_creature =
        Program::creature(Some(green_cost), 2.into(), 2.into(), BTreeSet::new());
    legendary_creature.supertypes.insert(Supertype::Legendary);

    let source = main_state(vec![
        card(1, A, ObjectDestination::Hand, producer),
        card(2, A, ObjectDestination::Hand, nonlegendary_creature),
        card(3, A, ObjectDestination::Hand, legendary_creature),
    ]);
    let cast_prod = bridged(&source, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let funded = resolve(&cast_prod);

    // Verify wire projection includes both predicate restricted mana entries.
    let mut view = scope(&funded, viewer_a());
    let snap = super::projection::project_test(&mut view).unwrap();
    let seat_a = snap
        .players
        .iter()
        .find(|s| s.seat_id == viewer_a())
        .unwrap();
    assert_eq!(seat_a.restricted_mana.len(), 2);
    let red_wire = seat_a
        .restricted_mana
        .iter()
        .find(|r| r.color == ManaColor::Red)
        .expect("red predicate mana");
    assert!(red_wire.restriction == PlayerManaRestriction::Predicate);
    assert!(red_wire.predicate.as_ref().unwrap().disallow_generic);
    let green_wire = seat_a
        .restricted_mana
        .iter()
        .find(|r| r.color == ManaColor::Green)
        .expect("green predicate mana");
    assert!(green_wire.restriction == PlayerManaRestriction::Predicate);
    assert!(
        green_wire
            .predicate
            .as_ref()
            .unwrap()
            .makes_spell_uncounterable
    );

    // Atomic rejection: trying to spend 2 no-generic {R} on {1}{R} must fail without mutating state.
    let bad_payment = PlayerPayment {
        from_lots: BTreeMap::new(),
        graveyard_permission: None,
        use_granted_flashback: false,
        exile_cast_type: None,
        alternative_cost_source: None,
        spend: vec![ManaSpend {
            color: ManaColor::Red,
            amount: rc("2"),
        }],
        restricted_mana: vec![PlayerRestrictedManaSpend {
            color: ManaColor::Red,
            restriction: PlayerManaRestriction::Predicate,
            creature_type: None,
            min_mana_value: None,
            predicate: red_wire.predicate.clone(),
            amount: rc("2"),
        }],
    };
    let mut bad_view = scope(&funded, viewer_a());
    let bad_handle = bad_view
        .issue(NativeReference::Object(object(&funded, 2)))
        .unwrap();
    let bad_intent = command(
        &bad_view,
        PlayerAction::CastUntargeted {
            source: bad_handle.clone(),
            payment: Some(bad_payment),
        },
    );
    let _ = rejected_proposal(&bad_view, &bad_intent);

    // Atomic rejection on malformed empty wire predicate.
    let malformed = command(
        &bad_view,
        PlayerAction::CastUntargeted {
            source: bad_handle,
            payment: Some(PlayerPayment {
                from_lots: BTreeMap::new(),
                graveyard_permission: None,
                use_granted_flashback: false,
                exile_cast_type: None,
                alternative_cost_source: None,
                spend: vec![ManaSpend {
                    color: ManaColor::Green,
                    amount: rc("1"),
                }],
                restricted_mana: vec![PlayerRestrictedManaSpend {
                    color: ManaColor::Green,
                    restriction: PlayerManaRestriction::Predicate,
                    creature_type: None,
                    min_mana_value: None,
                    predicate: Some(crate::player_wire::PlayerManaPredicate::default()),
                    amount: rc("1"),
                }],
            }),
        },
    );
    let _ = rejected_proposal(&bad_view, &malformed);

    // Legal spend: casting legendary creature with legendary-predicate {G} succeeds.
    let good_payment = PlayerPayment {
        from_lots: BTreeMap::new(),
        graveyard_permission: None,
        use_granted_flashback: false,
        exile_cast_type: None,
        alternative_cost_source: None,
        spend: vec![ManaSpend {
            color: ManaColor::Green,
            amount: rc("1"),
        }],
        restricted_mana: vec![PlayerRestrictedManaSpend {
            color: ManaColor::Green,
            restriction: PlayerManaRestriction::Predicate,
            creature_type: None,
            min_mana_value: None,
            predicate: green_wire.predicate.clone(),
            amount: rc("1"),
        }],
    };
    let cast_leg = bridged(&funded, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&funded, 3)))
            .unwrap(),
        payment: Some(good_payment),
    });
    let resolved = resolve(&cast_leg);
    assert_eq!(
        resolved
            .execution()
            .window()
            .cards()
            .get(object(&resolved, 3))
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn populate_auto_resolves_singleton_prompts_on_multiple_and_rejects_invalid_atomically() {
    use crate::player_wire::{DecisionKind, PlayerDomain};
    use spellwake_engine::{
        effects::ResolutionSelectionKind,
        object_identity::ObjectZone,
        token::{TokenCreation, TokenDefinition, TokenName},
    };

    let vocab = vocabulary();
    let make_token = |p: i64, t: i64| {
        TokenDefinition::new(
            Program::creature(None, p.into(), t.into(), BTreeSet::new()),
            TokenName::Default {
                ordered_subtypes: vec![],
            },
            &vocab,
        )
        .unwrap()
    };
    // First verify deterministic singleton auto-resolution (1 creature token -> auto-copies without prompting).
    let singleton_spell = instant(
        BodyTarget::None,
        vec![
            Instruction::CreateTokens {
                creations: vec![TokenCreation::one(make_token(1, 1))],
            },
            Instruction::Populate,
        ],
    );
    let singleton_source = main_state(vec![card(1, A, ObjectDestination::Hand, singleton_spell)]);
    let singleton_cast = bridged(&singleton_source, viewer_a(), |v| {
        PlayerAction::CastUntargeted {
            source: v
                .issue(NativeReference::Object(object(&singleton_source, 1)))
                .unwrap(),
            payment: None,
        }
    });
    let singleton_done = resolve(&singleton_cast);
    assert!(singleton_done.pending_decision().is_none());
    assert_eq!(
        singleton_done
            .execution()
            .window()
            .cards()
            .iter()
            .filter(|c| c.zone() == ObjectZone::Battlefield && c.is_token())
            .count(),
        2
    );

    // Multi-token Populate: 2 creature tokens + 1 nontoken creature -> prompts over the 2 tokens only.
    let setup_spell = instant(
        BodyTarget::None,
        vec![
            Instruction::CreateTokens {
                creations: vec![TokenCreation::one(make_token(1, 1))],
            },
            Instruction::CreateTokens {
                creations: vec![TokenCreation::one(make_token(3, 3))],
            },
            Instruction::Populate,
        ],
    );
    let source = main_state(vec![
        card(1, A, ObjectDestination::Hand, setup_spell),
        card(
            2,
            A,
            ObjectDestination::Battlefield { controller: A },
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
    ]);
    let cast = bridged(&source, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let pending = resolve(&cast);

    // Must prompt with ResolutionSelectionKind::Populate over the 2 creature tokens (excluding the nontoken creature).
    let Some(RuleFrame::ResolutionSelect(frame)) = pending.pending_decision().map(|p| p.frame())
    else {
        panic!("expected ResolutionSelect decision for Populate");
    };
    assert_eq!(frame.operation(), ResolutionSelectionKind::Populate);
    assert_eq!(frame.domain().len(), 2);
    assert_eq!(frame.minimum(), 1);
    assert_eq!(frame.required(), 1);
    let nontoken = object(&pending, 2);
    assert!(!frame.domain().contains(&nontoken));

    // Verify wire projection emits SelectResolution with minimum=1, maximum=1.
    let mut view = scope(&pending, viewer_a());
    let snap = super::projection::project_test(&mut view).unwrap();
    assert_eq!(snap.decisions.len(), 1);
    assert_eq!(snap.decisions[0].kind, DecisionKind::SelectResolution);
    let PlayerDomain::Objects {
        objects,
        minimum,
        maximum,
        ..
    } = &snap.decisions[0].domain
    else {
        panic!("expected Objects domain for Populate");
    };
    assert_eq!(objects.len(), 2);
    assert_eq!(*minimum.as_rules_integer(), 1.into());
    assert_eq!(*maximum.as_rules_integer(), 1.into());

    // Atomic invalid reply: empty selection and nontoken creature must both fail without mutating state.
    let dec_handle = view
        .issue(NativeReference::Decision(
            pending.pending_decision().unwrap().id(),
        ))
        .unwrap();
    let empty_cmd = command(
        &view,
        PlayerAction::Choose {
            decision: dec_handle.clone(),
            response: PlayerChoice::Objects { objects: vec![] },
        },
    );
    let _ = rejected_proposal(&view, &empty_cmd);

    let nontoken_handle = view.issue(NativeReference::Object(nontoken)).unwrap();
    let bad_obj_cmd = command(
        &view,
        PlayerAction::Choose {
            decision: dec_handle,
            response: PlayerChoice::Objects {
                objects: vec![nontoken_handle],
            },
        },
    );
    let _ = rejected_proposal(&view, &bad_obj_cmd);

    // Legal reply: choose the second creature token and verify a 3rd creature token enters the battlefield.
    let chosen = frame.domain()[1];
    let done = reply(
        &pending,
        |v| PlayerChoice::Objects {
            objects: vec![v.issue(NativeReference::Object(chosen)).unwrap()],
        },
        DecisionReply::SelectResolution(vec![chosen]),
    );
    let token_count = done
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Battlefield && c.is_token())
        .count();
    assert_eq!(token_count, 3);
}

#[test]
#[ignore = "requires explicitly supplied source-qualified creature vocabulary"]
fn reveal_top_two_piles_fact_or_fiction_and_steam_augury_round_trip_and_reject_invalid_atomically()
{
    use crate::player_wire::{DecisionKind, PlayerDomain};
    use spellwake_engine::{
        object_identity::ObjectZone,
        program::{LookTopKeepDest, LookTopRestDest, ValueExpr},
    };

    // Steam Augury mode: controller (A) divides top 3 cards into two piles, opponent (B) chooses a pile
    // -> chosen pile goes to A's hand, other pile goes to A's graveyard.
    let steam_augury = instant(
        BodyTarget::None,
        vec![Instruction::RevealTopTwoPiles {
            count: ValueExpr::Literal(3.into()),
            controller_divides: true,
            target_opponent: false,
            chosen_dest: LookTopKeepDest::Hand,
            other_dest: LookTopRestDest::Graveyard,
        }],
    );
    let vanilla = || Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    let source = main_state(vec![
        card(1, A, ObjectDestination::Hand, steam_augury),
        card(2, A, ObjectDestination::LibraryTop, vanilla()),
        card(3, A, ObjectDestination::LibraryTop, vanilla()),
        card(4, A, ObjectDestination::LibraryTop, vanilla()),
    ]);
    let cast = bridged(&source, viewer_a(), |v| PlayerAction::CastUntargeted {
        source: v
            .issue(NativeReference::Object(object(&source, 1)))
            .unwrap(),
        payment: None,
    });
    let divide_pending = resolve(&cast);

    // Controller A divides the 3 revealed cards.
    let dec = divide_pending.pending_decision().expect("divide decision");
    assert_eq!(dec.chooser(), A);
    let RuleFrame::RevealedPiles(frame) = dec.frame() else {
        panic!("expected RevealedPiles divide frame");
    };
    let spellwake_engine::effects::PileChoice::Divide(cards) = frame.choice() else {
        panic!("expected PileChoice::Divide");
    };
    assert_eq!(cards.len(), 3);
    let first_pile = vec![cards[0]];

    // Atomic rejection of out-of-domain card in Divide.
    let mut view_a = scope(&divide_pending, viewer_a());
    let snap_a = super::projection::project_test(&mut view_a).unwrap();
    assert_eq!(snap_a.decisions.len(), 1);
    assert_eq!(snap_a.decisions[0].kind, DecisionKind::LibrarySearch);
    assert!(matches!(
        snap_a.decisions[0].domain,
        PlayerDomain::LibrarySearch {
            mode: crate::player_wire::PlayerSearchMode::DividePile,
            ..
        }
    ));
    let dec_handle_a = view_a.issue(NativeReference::Decision(dec.id())).unwrap();
    // Atomic rejection of out-of-domain object (e.g., an object not among the 3 revealed cards).
    let bad_obj = view_a
        .issue(NativeReference::Object(object(&divide_pending, 1)))
        .unwrap();
    let bad_divide = command(
        &view_a,
        PlayerAction::Choose {
            decision: dec_handle_a,
            response: PlayerChoice::Objects {
                objects: vec![bad_obj],
            },
        },
    );
    let _ = rejected_proposal(&view_a, &bad_divide);

    let take_pending = reply(
        &divide_pending,
        |v| PlayerChoice::Objects {
            objects: vec![v.issue(NativeReference::Object(first_pile[0])).unwrap()],
        },
        DecisionReply::SelectResolution(first_pile.clone()),
    );

    // Opponent B chooses pile 1 or pile 2.
    let take_dec = take_pending.pending_decision().expect("take decision");
    assert_eq!(take_dec.chooser(), B);
    let mut view_b = scope(&take_pending, viewer_b());
    let dec_handle_b = view_b
        .issue(NativeReference::Decision(take_dec.id()))
        .unwrap();
    // Atomic rejection of invalid pile index 3.
    let bad_pile = command(
        &view_b,
        PlayerAction::Choose {
            decision: dec_handle_b,
            response: PlayerChoice::Number { value: 3.into() },
        },
    );
    let _ = rejected_proposal(&view_b, &bad_pile);

    // Legal reply: B chooses pile 1 (1 card -> A's hand; 2 cards -> A's graveyard).
    let after_take = bridged(&take_pending, viewer_b(), |v| PlayerAction::Choose {
        decision: v.issue(NativeReference::Decision(take_dec.id())).unwrap(),
        response: PlayerChoice::Number { value: 1.into() },
    });
    // Complete graveyard ordering if 2 cards enter graveyard simultaneously.
    let done = if let Some(order_dec) = after_take.pending_decision() {
        let RuleFrame::RevealedPiles(ord_frame) = order_dec.frame() else {
            panic!("expected graveyard order frame");
        };
        let spellwake_engine::effects::PileChoice::GraveyardOrder(gy_cards) = ord_frame.choice()
        else {
            panic!("expected PileChoice::GraveyardOrder");
        };
        let gy_vec = gy_cards.clone();
        reply(
            &after_take,
            |v| PlayerChoice::Ordering {
                ordered: gy_vec
                    .iter()
                    .map(|id| v.issue(NativeReference::Object(*id)).unwrap())
                    .collect(),
            },
            DecisionReply::OrderResolutionGraveyard(gy_vec.clone()),
        )
    } else {
        after_take
    };
    let hand_count = done
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Hand(A))
        .count();
    let gy_count = done
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|c| c.zone() == ObjectZone::Graveyard(A))
        .count();
    assert_eq!(hand_count, 1);
    assert!(gy_count >= 2);
}
