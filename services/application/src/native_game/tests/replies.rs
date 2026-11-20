//! Actual kernel decision frames, with exact translated input and atomic source checks.
use super::*;
use crate::player_wire::PlayerChoice;
use spellwake_engine::{
    effects::OptionalEffectChoice,
    game::{DecisionReply, RuleFrame},
    program::{
        AffectedPlayers, CardType, CreatureTypeSlot, InstructionAmount, PermanentTypeFilter,
        ResolvingPlayer,
    },
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
            amount: InstructionAmount::Literal(2.into()),
            filter: PermanentTypeFilter {
                required_types: BTreeSet::from([CardType::Creature]),
                forbidden_types: BTreeSet::new(),
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
    use spellwake_engine::{effects::CopyTargetChoice, program::SpellTargetFilter};
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
                BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
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
        OrdinaryCondition, PlayerRelation, SpellCastFilter, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId,
    };
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered = (1..=2)
        .map(|id| TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: OrdinaryCondition::SpellCast {
                player: PlayerRelation::Controller,
                filter: SpellCastFilter::AnySpell,
            },
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
                        DamageRecipient::Creature(id) => PlayerTarget::Object {
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
