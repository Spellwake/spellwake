//! Actual projected options and retained originals over a genuine-vocabulary mechanism.
use super::*;
use spellwake_engine::{
    effects::{DecisionReply, SeedCard},
    game::{GameInput, RuleFrame},
    program::{
        ObjectRelation, OrdinaryCondition, ResolvingPlayer, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId,
    },
    setup::StartingLife,
    turn::{Step, TurnStart},
};
const C: SeatId = SeatId(29);
const D: SeatId = SeatId(77);
fn app_c() -> Uuid {
    Uuid::from_u128(301)
}
fn app_d() -> Uuid {
    Uuid::from_u128(302)
}
fn source() -> GameState {
    source_with_extra(false)
}
fn source_with_extra(extra: bool) -> GameState {
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::LeavesBattlefield(ObjectRelation::AnyCreature),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OptionalEffect {
                    chooser: ResolvingPlayer::UniqueRelatedPlayer,
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(7.into()),
                },
            ],
        ),
    });
    let mut seed = seed(vec![
        card(
            1,
            B,
            ObjectDestination::Battlefield { controller: A },
            watcher,
        ),
        card(
            2,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
        ),
    ]);
    seed.seats.extend([C, D]);
    seed.life.extend([
        StartingLife {
            player: C,
            total: 20.into(),
        },
        StartingLife {
            player: D,
            total: 20.into(),
        },
    ]);
    if extra {
        seed.seats.push(SeatId(88));
        seed.life.push(StartingLife {
            player: SeatId(88),
            total: 20.into(),
        });
    }
    let mut state = GameState::initialize_scheduled(
        GameId(90),
        context(&seed.creature_vocabulary),
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    for (actor, command) in [
        (B, GameCommand::Concede),
        (A, GameCommand::Pass),
        (C, GameCommand::Pass),
        (D, GameCommand::Pass),
    ] {
        state = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            })
            .unwrap()
            .into_state();
    }
    if extra {
        state = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor: SeatId(88),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ReplacementChooser(_)
    ));
    state
}
pub(super) fn bound(source: &GameState, viewer: Uuid) -> NativeBinding {
    // Deliberately differs from both seed order [A,B,C,D] and live [A,C,D].
    let mut seats = vec![
        NativeSeatBinding {
            seat_id: app_d(),
            native_seat: D,
        },
        NativeSeatBinding {
            seat_id: viewer_a(),
            native_seat: A,
        },
        NativeSeatBinding {
            seat_id: viewer_b(),
            native_seat: B,
        },
        NativeSeatBinding {
            seat_id: app_c(),
            native_seat: C,
        },
    ];
    if source.original_participants().contains(&SeatId(88)) {
        seats.push(NativeSeatBinding {
            seat_id: Uuid::from_u128(303),
            native_seat: SeatId(88),
        });
    }
    NativeBinding::new(context_id(), source.basis().clone(), seats, viewer).unwrap()
}
fn projected(scope: &mut NativeScope<'_>) -> NativeProjectedSnapshot {
    let projected = scope
        .project_with_facts(&metadata(), &Display::default())
        .unwrap();
    let facts = projected
        .facts(scope.source.execution().creature_vocabulary())
        .unwrap();
    projected.snapshot().validate(&facts).unwrap();
    let decoded: PlayerSnapshot =
        crate::decode_wire_object(&serde_json::to_vec(projected.snapshot()).unwrap()).unwrap();
    decoded.validate(&facts).unwrap();
    capture(projected.snapshot());
    projected
}

#[test]
#[ignore = "requires genuine vocabulary; finite chooser public/private codec composition"]
fn replacement_options_roundtrip_original_player_mapping_then_selected_survivor_optional_choice() {
    let source = source();
    let before = source.clone();
    let mut view = NativeScope::new(&source, bound(&source, viewer_a())).unwrap();
    let snapshot = projected(&mut view);
    let descriptor = &snapshot.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::ReplacementChooser);
    assert_eq!(descriptor.responding_seat_ids, vec![viewer_a()]);
    let PlayerDomain::Options {
        options,
        query: None,
    } = &descriptor.domain
    else {
        panic!("replacement options")
    };
    assert_eq!(options.len(), 2);
    let chosen = options
        .iter()
        .find(|option| {
            view.reference(&option.handle).unwrap() == NativeReference::ReplacementChooser(D)
        })
        .unwrap()
        .handle
        .clone();
    let command = command(
        &view,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response: PlayerChoice::Option {
                option: chosen.clone(),
            },
        },
    );
    descriptor
        .validate_response(
            &command,
            viewer_a(),
            &snapshot
                .snapshot()
                .players
                .iter()
                .map(|seat| seat.seat_id)
                .collect::<Vec<_>>(),
            &[],
        )
        .unwrap();
    let prepared = view.propose(&command).unwrap();
    input_codec::verify(&view, &prepared);
    let bytes = view.encode_input(&prepared).unwrap();
    let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let location = record["transition"]["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["handle"] == serde_json::to_value(&chosen).unwrap())
        .unwrap();
    assert_eq!(location["location"]["kind"], "replacement-chooser");
    assert_eq!(location["location"]["target"]["kind"], "player");
    assert_eq!(
        location["location"]["target"]["index"], "0",
        "use original binding mapping, not live or seed indices"
    );
    for target in [
        serde_json::json!({"kind":"stack","index":"0"}),
        serde_json::json!({"kind":"player","index":"2"}),
        serde_json::json!({"kind":"player","index":"999"}),
    ] {
        let mut wrong = record.clone();
        let location = wrong["transition"]["references"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["handle"] == serde_json::to_value(&chosen).unwrap())
            .unwrap();
        location["location"]["target"] = target;
        assert!(
            restore_input(
                &source,
                view.binding.clone(),
                &serde_json::to_vec(&wrong).unwrap()
            )
            .is_err()
        );
    }
    let next = prepared.candidate().state();
    assert_eq!(next.pending_decision().unwrap().chooser(), D);
    assert!(matches!(
        next.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    let mut observer = NativeScope::new(&source, bound(&source, app_c())).unwrap();
    assert!(projected(&mut observer).snapshot().decisions.is_empty());
    assert!(observer.reference(&chosen).is_err());
    let mut next_view = NativeScope::new(next, bound(next, app_d())).unwrap();
    let next_snapshot = projected(&mut next_view);
    let descriptor = &next_snapshot.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::OptionalEffect);
    let PlayerDomain::Options { options, .. } = &descriptor.domain else {
        panic!("optional options")
    };
    let take = options
        .iter()
        .find(|option| {
            next_view.reference(&option.handle).unwrap()
                == NativeReference::OptionalEffect(
                    spellwake_engine::effects::OptionalEffectChoice::Take,
                )
        })
        .unwrap()
        .handle
        .clone();
    let command = crate::native_game::tests::fixture::command(
        &next_view,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response: PlayerChoice::Option { option: take },
        },
    );
    assert!(next_view.reference(&chosen).is_err());
    let prepared = next_view.propose(&command).unwrap();
    input_codec::verify(&next_view, &prepared);
    assert_eq!(
        prepared.candidate().state().execution().life_total(A),
        Some(&27.into())
    );
    assert_eq!(source, before);
}

fn concede_original(source: &GameState, actor: Uuid) -> GameState {
    let scope = NativeScope::new(source, bound(source, actor)).unwrap();
    let prepared = scope
        .propose(&command(&scope, PlayerAction::Concede {}))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    prepared.candidate().state().clone()
}
fn select_replacement(source: &GameState, selected: SeatId) -> GameState {
    let mut scope = NativeScope::new(source, bound(source, viewer_a())).unwrap();
    let snapshot = projected(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    let PlayerDomain::Options { options, .. } = &descriptor.domain else {
        panic!()
    };
    let handle = options
        .iter()
        .find(|option| {
            scope.reference(&option.handle).unwrap()
                == NativeReference::ReplacementChooser(selected)
        })
        .unwrap()
        .handle
        .clone();
    let prepared = scope
        .propose(&command(
            &scope,
            PlayerAction::Choose {
                decision: descriptor.decision.clone(),
                response: PlayerChoice::Option { option: handle },
            },
        ))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    prepared.candidate().state().clone()
}
#[test]
#[ignore = "requires genuine vocabulary; pending optional departure normalized-original replay"]
fn pending_optional_departure_originals_refresh_candidates_replaced_chooser_and_ceased_entry() {
    let source = source_with_extra(true);
    let mut old_view = NativeScope::new(&source, bound(&source, viewer_a())).unwrap();
    let old_snapshot = projected(&mut old_view);
    let old_decision = old_snapshot.snapshot().decisions[0].decision.clone();
    let PlayerDomain::Options { options, .. } = &old_snapshot.snapshot().decisions[0].domain else {
        panic!()
    };
    let old_handle = options[0].handle.clone();
    let refreshed = concede_original(&source, Uuid::from_u128(303));
    assert_eq!(refreshed.execution().window().participants(), &[A, C, D]);
    let mut refreshed_view = NativeScope::new(&refreshed, bound(&refreshed, viewer_a())).unwrap();
    let snapshot = projected(&mut refreshed_view);
    assert!(snapshot.snapshot().decisions[0].decision != old_decision);
    assert!(refreshed_view.reference(&old_handle).is_err());
    let selected = select_replacement(&refreshed, D);
    let departed = concede_original(&selected, app_d());
    let RuleFrame::ReplacementChooser(frame) = departed.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.departed(), D);
    assert_eq!(frame.candidates(), &[C]);
    let selected = select_replacement(&departed, C);
    let mut scope = NativeScope::new(&selected, bound(&selected, app_c())).unwrap();
    let snapshot = projected(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    let PlayerDomain::Options { options, .. } = &descriptor.domain else {
        panic!()
    };
    let take = options
        .iter()
        .find(|option| {
            scope.reference(&option.handle).unwrap()
                == NativeReference::OptionalEffect(
                    spellwake_engine::effects::OptionalEffectChoice::Take,
                )
        })
        .unwrap()
        .handle
        .clone();
    let prepared = scope
        .propose(&command(
            &scope,
            PlayerAction::Choose {
                decision: descriptor.decision.clone(),
                response: PlayerChoice::Option { option: take },
            },
        ))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    assert_eq!(
        prepared.candidate().state().execution().life_total(A),
        Some(&27.into())
    );
    // The controller leaves while its entry is still awaiting substitution.
    let ceased = concede_original(&source, viewer_a());
    assert!(ceased.pending_decision().is_none());
    assert!(ceased.execution().window().entries().is_empty());
    assert_eq!(ceased.execution().life_total(A), Some(&20.into()));
    let mut observer = NativeScope::new(&ceased, bound(&ceased, app_c())).unwrap();
    assert!(projected(&mut observer).snapshot().decisions.is_empty());
}

fn payment_source() -> GameState {
    payment_source_with_color(false)
}
fn payment_source_with_color(color: bool) -> GameState {
    let body = ExecutableBody::new(
        BodyTarget::SinglePlayer,
        vec![
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(2.into()),
            },
            Instruction::OfferManaPayment {
                payer: ResolvingPlayer::TargetPlayer,
                cost: ManaCost::free(),
                paid_destination: 2,
                declined_destination: 4,
            },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(7.into()),
            },
            Instruction::Jump { destination: 5 },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            },
        ],
    );
    let mut cards = vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::instant(ManaCost::free(), body),
    )];
    if color {
        cards.push(card(
            70,
            C,
            ObjectDestination::Battlefield { controller: B },
            Program::land(
                BTreeSet::new(),
                vec![spellwake_engine::mana::ManaAbility {
                    id: spellwake_engine::mana::ManaAbilityId(7),
                    output: spellwake_engine::mana::ManaOutput::ChooseColor {
                        amount: 1.into(),
                        colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
                    },
                }],
            ),
        ));
    }
    let mut seed = seed(cards);
    seed.seats.extend([C, D]);
    seed.life.extend([
        StartingLife {
            player: C,
            total: 20.into(),
        },
        StartingLife {
            player: D,
            total: 20.into(),
        },
    ]);
    let mut state = GameState::initialize_scheduled(
        GameId(90),
        context(&seed.creature_vocabulary),
        seed,
        TurnStart {
            number: 1,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    for (actor, command) in [
        (
            A,
            GameCommand::Cast {
                spell,
                target: TargetRef::Player(B),
            },
        ),
        (A, GameCommand::Pass),
        (B, GameCommand::Pass),
        (C, GameCommand::Pass),
        (D, GameCommand::Pass),
    ] {
        state = state
            .advance(GameInput {
                basis: state.basis().clone(),
                actor,
                command,
            })
            .unwrap()
            .into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    state
}
#[test]
#[ignore = "requires genuine vocabulary; direct payment departure normalized-original replay"]
fn pending_payment_departure_originals_preserve_quote_or_complete_authored_unpaid_branch() {
    let source = payment_source();
    let unpaid = concede_original(&source, viewer_b());
    assert!(unpaid.pending_decision().is_none());
    assert_eq!(unpaid.execution().life_total(A), Some(&25.into()));
    let mut view = NativeScope::new(&unpaid, bound(&unpaid, viewer_a())).unwrap();
    assert!(projected(&mut view).snapshot().decisions.is_empty());
    let waiting = concede_original(&source, app_c());
    let mut view = NativeScope::new(&waiting, bound(&waiting, viewer_b())).unwrap();
    let snapshot = projected(&mut view);
    let descriptor = &snapshot.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::ResolutionPayment);
    let prepared = view
        .propose(&command(
            &view,
            PlayerAction::Choose {
                decision: descriptor.decision.clone(),
                response: PlayerChoice::Decline {},
            },
        ))
        .unwrap();
    input_codec::verify(&view, &prepared);
    assert_eq!(
        prepared.candidate().state().execution().life_total(A),
        Some(&25.into())
    );
    let ceased = concede_original(&source, viewer_a());
    assert!(ceased.pending_decision().is_none());
    assert_eq!(ceased.execution().life_total(A), Some(&22.into()));
}

fn color_source() -> GameState {
    let state = payment_source_with_color(true);
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(70))
        .unwrap();
    state
        .advance(GameInput {
            basis: state.basis().clone(),
            actor: B,
            command: GameCommand::ActivateMana {
                source,
                ability: ManaAbilityRef::Printed(spellwake_engine::mana::ManaAbilityId(7)),
            },
        })
        .unwrap()
        .into_state()
}
#[test]
#[ignore = "requires genuine vocabulary; retained mana child departure normalized-original replay"]
fn mana_color_departure_originals_keep_gone_source_or_orphaned_parent_and_credit_once() {
    let source = color_source();
    for departed in [app_c(), viewer_a()] {
        let mut old_view = NativeScope::new(&source, bound(&source, viewer_b())).unwrap();
        let old = projected(&mut old_view);
        let PlayerDomain::Options { options, .. } = &old.snapshot().decisions[0].domain else {
            panic!()
        };
        let old_handle = options[0].handle.clone();
        let waiting = concede_original(&source, departed);
        let mut view = NativeScope::new(&waiting, bound(&waiting, viewer_b())).unwrap();
        let snapshot = projected(&mut view);
        let descriptor = &snapshot.snapshot().decisions[0];
        assert!(descriptor.kind == DecisionKind::ManaColor);
        assert!(view.reference(&old_handle).is_err());
        let PlayerDomain::Options { options, .. } = &descriptor.domain else {
            panic!()
        };
        let red = options
            .iter()
            .find(|option| {
                view.reference(&option.handle).unwrap() == NativeReference::ManaColor(ManaType::Red)
            })
            .unwrap()
            .handle
            .clone();
        let prepared = view
            .propose(&command(
                &view,
                PlayerAction::Choose {
                    decision: descriptor.decision.clone(),
                    response: PlayerChoice::Option { option: red },
                },
            ))
            .unwrap();
        input_codec::verify(&view, &prepared);
        let after = prepared.candidate().state();
        assert_eq!(after.execution().mana(B, ManaType::Red), 1.into());
        let mut after_view = NativeScope::new(after, bound(after, viewer_b())).unwrap();
        let snapshot = projected(&mut after_view);
        if departed == viewer_a() {
            assert!(after.pending_decision().is_none());
            assert!(snapshot.snapshot().decisions.is_empty());
            assert!(after.execution().window().entries().is_empty());
            assert_eq!(after.execution().life_total(A), Some(&22.into()));
        } else {
            let descriptor = &snapshot.snapshot().decisions[0];
            assert!(descriptor.kind == DecisionKind::ResolutionPayment);
            let declined = after_view
                .propose(&command(
                    &after_view,
                    PlayerAction::Choose {
                        decision: descriptor.decision.clone(),
                        response: PlayerChoice::Decline {},
                    },
                ))
                .unwrap();
            input_codec::verify(&after_view, &declined);
            assert_eq!(
                declined.candidate().state().execution().life_total(A),
                Some(&25.into())
            );
        }
    }
    let unpaid = concede_original(&source, viewer_b());
    assert!(unpaid.pending_decision().is_none());
    assert_eq!(unpaid.execution().life_total(A), Some(&25.into()));
}

pub(super) fn four_seat_root(cards: Vec<SeedCard>) -> GameState {
    four_seat_root_at(cards, Step::PrecombatMain)
}
pub(super) fn four_seat_root_at(cards: Vec<SeedCard>, step: Step) -> GameState {
    let mut seed = seed(cards);
    seed.seats.extend([C, D]);
    seed.life.extend([
        StartingLife {
            player: C,
            total: 20.into(),
        },
        StartingLife {
            player: D,
            total: 20.into(),
        },
    ]);
    GameState::initialize_scheduled(
        GameId(90),
        context(&seed.creature_vocabulary),
        seed,
        TurnStart { number: 1, step },
    )
    .unwrap()
    .into_state()
}
pub(super) fn mechanism_step(source: &GameState, actor: SeatId, command: GameCommand) -> GameState {
    source
        .advance(GameInput {
            basis: source.basis().clone(),
            actor,
            command,
        })
        .unwrap()
        .into_state()
}
fn cursor_type_source() -> GameState {
    use spellwake_engine::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    use spellwake_engine::program::CreatureTypeSlot;
    let body = ExecutableBody::new(
        BodyTarget::SinglePlayer,
        vec![
            Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(2.into()),
            },
            Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(1),
                excluding: BTreeSet::new(),
            },
            Instruction::AdjustTargetLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            },
        ],
    );
    let mut state = four_seat_root(vec![card(
        1,
        B,
        ObjectDestination::Battlefield { controller: A },
        Program::artifact(
            None,
            vec![ActivatedAbility {
                id: ActivatedAbilityId(1),
                mana_cost: ManaCost::free(),
                source_cost: SourceCost::None,
                body,
            }],
        ),
    )]);
    let source = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state = mechanism_step(
        &state,
        A,
        GameCommand::ActivateAbility {
            source,
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(A),
            payment: None,
        },
    );
    for actor in [A, B, C, D] {
        state = mechanism_step(&state, actor, GameCommand::Pass);
    }
    let human = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    mechanism_step(
        &state,
        A,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::CreatureType(human),
        },
    )
}
fn cursor_copy_source() -> GameState {
    let mut state = four_seat_root(vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleSpell(
                        spellwake_engine::program::SpellTargetFilter::CopyableInstructionSpell,
                    ),
                    vec![
                        Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(2.into()),
                        },
                        Instruction::CopySpell,
                        Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(3.into()),
                        },
                    ],
                ),
            ),
        ),
        card(
            2,
            B,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SinglePlayer,
                    vec![Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Lose(4.into()),
                    }],
                ),
            ),
        ),
    ]);
    state = mechanism_step(&state, A, GameCommand::Pass);
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(2))
        .unwrap();
    state = mechanism_step(
        &state,
        B,
        GameCommand::Cast {
            spell,
            target: TargetRef::Player(C),
        },
    );
    let original = state.execution().window().entries()[0].id();
    for actor in [B, C, D] {
        state = mechanism_step(&state, actor, GameCommand::Pass);
    }
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state = mechanism_step(
        &state,
        A,
        GameCommand::Cast {
            spell,
            target: TargetRef::Stack(original),
        },
    );
    for actor in [A, B, C, D] {
        state = mechanism_step(&state, actor, GameCommand::Pass);
    }
    state
}
fn choose_reference(source: &GameState, actor: Uuid, expected: NativeReference) -> GameState {
    let mut scope = NativeScope::new(source, bound(source, actor)).unwrap();
    let snapshot = projected(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    let PlayerDomain::Options { options, .. } = &descriptor.domain else {
        panic!()
    };
    let handle = options
        .iter()
        .find(|option| scope.reference(&option.handle).unwrap() == expected)
        .unwrap()
        .handle
        .clone();
    let prepared = scope
        .propose(&command(
            &scope,
            PlayerAction::Choose {
                decision: descriptor.decision.clone(),
                response: PlayerChoice::Option { option: handle },
            },
        ))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    prepared.candidate().state().clone()
}
pub(super) fn finish_original_stack(mut source: GameState) -> GameState {
    for _ in 0..30 {
        if source.execution().window().entries().is_empty() {
            return source;
        }
        let spellwake_engine::stack_window::WindowPhase::Priority(
            spellwake_engine::priority::PriorityPhase::Ready { holder },
        ) = source.execution().window().phase()
        else {
            panic!()
        };
        let binding = bound(&source, viewer_a());
        let viewer = binding
            .seats
            .iter()
            .find(|seat| seat.native_seat == holder)
            .unwrap()
            .seat_id;
        let scope = NativeScope::new(&source, bound(&source, viewer)).unwrap();
        let prepared = scope
            .propose(&command(&scope, PlayerAction::Pass {}))
            .unwrap();
        input_codec::verify(&scope, &prepared);
        source = prepared.candidate().state().clone();
    }
    panic!("bounded native stack progress")
}
#[test]
#[ignore = "requires genuine vocabulary; plain cursor departure and actual normalized-original replay"]
fn resolution_cursor_departure_originals_preserve_locals_and_illegal_retain_then_resolve_stack() {
    use spellwake_engine::effects::CopyTargetChoice;
    let source = cursor_type_source();
    let saved = source.execution().resolution_state().unwrap();
    assert_eq!(saved.locals().len(), 1);
    let after = concede_original(&source, viewer_b());
    assert_eq!(after.execution().resolution_state(), Some(saved));
    let elf = after
        .execution()
        .creature_vocabulary()
        .resolve_label("Elf")
        .unwrap();
    let done = choose_reference(&after, viewer_a(), NativeReference::CreatureType(elf));
    assert_eq!(done.execution().life_total(A), Some(&25.into()));
    let source = cursor_copy_source();
    let mut old_scope = NativeScope::new(&source, bound(&source, viewer_a())).unwrap();
    let old = projected(&mut old_scope);
    let PlayerDomain::Options { options, .. } = &old.snapshot().decisions[0].domain else {
        panic!()
    };
    let old_handle = options[0].handle.clone();
    let after = concede_original(&source, app_c());
    let mut scope = NativeScope::new(&after, bound(&after, viewer_a())).unwrap();
    let snapshot = projected(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::CopyTargets);
    assert!(scope.reference(&old_handle).is_err());
    let PlayerDomain::Options { options, .. } = &descriptor.domain else {
        panic!()
    };
    assert!(
        !options
            .iter()
            .any(|option| scope.reference(&option.handle).unwrap()
                == NativeReference::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(C))))
    );
    for (choice, total) in [
        (CopyTargetChoice::Retain, 25),
        (CopyTargetChoice::Change(TargetRef::Player(A)), 21),
    ] {
        let copied = choose_reference(&after, viewer_a(), NativeReference::CopyTargets(choice));
        let done = finish_original_stack(copied);
        assert_eq!(done.execution().life_total(A), Some(&total.into()));
        assert_eq!(done.execution().life_total(C), Some(&20.into()));
    }
    let skipped = concede_original(&source, viewer_b());
    assert!(skipped.pending_decision().is_none());
    assert!(skipped.execution().window().entries().is_empty());
    assert_eq!(skipped.execution().life_total(A), Some(&25.into()));
    let canceled = concede_original(&source, viewer_a());
    let done = finish_original_stack(canceled);
    assert_eq!(done.execution().life_total(C), Some(&16.into()));
}

fn token_source() -> GameState {
    use spellwake_engine::token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients};
    let vocabulary = crate::native_game::tests::fixture::vocabulary();
    let creations = [1, 2]
        .into_iter()
        .map(|power| TokenCreation {
            definition: TokenDefinition::new(
                Program::creature(None, power.into(), 3.into(), BTreeSet::new()),
                TokenName::Default {
                    ordered_subtypes: vec![],
                },
                &vocabulary,
            )
            .unwrap(),
            amount: spellwake_engine::program::InstructionAmount::Literal(1.into()),
            recipients: TokenRecipients::EachPlayer,
        })
        .collect();
    let mut state = four_seat_root(vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(2.into()),
                    },
                    Instruction::CreateTokens { creations },
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    },
                ],
            ),
        ),
    )]);
    let spell = state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state = mechanism_step(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
    );
    for actor in [A, B, C, D] {
        state = mechanism_step(&state, actor, GameCommand::Pass);
    }
    state
}
fn order_original(source: &GameState, viewer: Uuid, check_sparse: bool) -> GameState {
    let mut scope = NativeScope::new(source, bound(source, viewer)).unwrap();
    let snapshot = projected(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    assert!(descriptor.kind == DecisionKind::TimestampOrder);
    let PlayerDomain::Ordering { objects, .. } = &descriptor.domain else {
        panic!()
    };
    let prepared = scope
        .propose(&command(
            &scope,
            PlayerAction::Choose {
                decision: descriptor.decision.clone(),
                response: PlayerChoice::Ordering {
                    ordered: objects.iter().rev().cloned().collect(),
                },
            },
        ))
        .unwrap();
    input_codec::verify(&scope, &prepared);
    let record: serde_json::Value =
        serde_json::from_slice(&scope.encode_input(&prepared).unwrap()).unwrap();
    let mut coordinates = std::collections::BTreeMap::new();
    for reference in record["transition"]["references"].as_array().unwrap() {
        if reference["location"]["kind"] == "token" {
            let handle: PlayerHandle = serde_json::from_value(reference["handle"].clone()).unwrap();
            let NativeReference::TokenMember(member) = scope.reference(&handle).unwrap() else {
                panic!()
            };
            let index = reference["location"]["index"]
                .as_str()
                .unwrap()
                .parse::<usize>()
                .unwrap();
            coordinates.insert(member.ordinal(), index);
        }
    }
    if check_sparse {
        assert_eq!(
            coordinates,
            std::collections::BTreeMap::from([(2, 0), (6, 1)])
        );
        let mut wrong = record.clone();
        let reference = wrong["transition"]["references"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|reference| reference["location"]["kind"] == "token")
            .unwrap();
        reference["location"]["index"] = serde_json::json!("6");
        assert!(
            restore_input(
                source,
                scope.binding.clone(),
                &serde_json::to_vec(&wrong).unwrap()
            )
            .is_err()
        );
    }
    prepared.candidate().state().clone()
}
#[test]
#[ignore = "requires genuine vocabulary; token sparse-member departure normalized-original replay"]
fn token_entry_departure_originals_keep_order_prefix_and_frame_local_indices_for_sparse_members() {
    let source = token_source();
    let accepted = order_original(&source, viewer_a(), false);
    let RuleFrame::TimestampOrder(frame) = accepted.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.chooser(), B);
    let mut old_view = NativeScope::new(&accepted, bound(&accepted, viewer_b())).unwrap();
    let old = projected(&mut old_view);
    let PlayerDomain::Ordering { objects, .. } = &old.snapshot().decisions[0].domain else {
        panic!()
    };
    let old_handle = objects[0].clone();
    let after = concede_original(&accepted, viewer_b());
    let RuleFrame::TimestampOrder(frame) = after.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.chooser(), C);
    assert_eq!(
        frame
            .members()
            .iter()
            .map(|member| member.member.ordinal())
            .collect::<Vec<_>>(),
        vec![2, 6]
    );
    assert!(
        after
            .execution()
            .window()
            .cards()
            .iter()
            .all(|object| !object.is_token())
    );
    let mut observer = NativeScope::new(&after, bound(&after, viewer_b())).unwrap();
    assert!(projected(&mut observer).snapshot().decisions.is_empty());
    let mut current = NativeScope::new(&after, bound(&after, app_c())).unwrap();
    projected(&mut current);
    assert!(current.reference(&old_handle).is_err());
    let after_c = order_original(&after, app_c(), true);
    assert!(
        after_c
            .execution()
            .window()
            .cards()
            .iter()
            .all(|object| !object.is_token())
    );
    let born = order_original(&after_c, app_d(), false);
    assert!(born.pending_decision().is_none());
    assert_eq!(born.execution().life_total(A), Some(&25.into()));
    let tokens = born
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|object| object.is_token())
        .collect::<Vec<_>>();
    assert_eq!(tokens.len(), 6);
    assert!(tokens.iter().all(|object| object.owner() != B));
    let timestamps = born
        .execution()
        .characteristic_timestamps()
        .map(|(id, timestamp)| (id, timestamp.sequence()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut ordered = tokens;
    ordered.sort_by_key(|object| timestamps[&object.id()]);
    let actual = ordered
        .iter()
        .map(|object| {
            (
                object.owner(),
                String::from(
                    born.execution()
                        .power_toughness(object.id())
                        .unwrap()
                        .unwrap()
                        .derived
                        .power,
                ),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (A, "2".into()),
            (A, "1".into()),
            (C, "2".into()),
            (C, "1".into()),
            (D, "2".into()),
            (D, "1".into())
        ]
    );
    let mut final_view = NativeScope::new(&born, bound(&born, viewer_a())).unwrap();
    projected(&mut final_view);
}
