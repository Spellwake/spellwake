//! Frozen token previews keep original ordinals across creator departure.
use super::*;
use crate::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
use crate::effects::OptionalEffectChoice;
use crate::token::{
    TokenBatchMember, TokenCreation, TokenDefinition, TokenName, TokenRecipients,
    TokenTimestampOrder,
};
const D: SeatId = SeatId(77);
#[derive(Default)]
struct Fixture {
    ability: bool,
    counted: bool,
    lower: bool,
    tail_choice: bool,
    controller: Option<SeatId>,
}
fn prepare(options: Fixture) -> GameState {
    let controller = options.controller.unwrap_or(A);
    let vocabulary = crate::test_support::creature_types::selected_vocabulary().unwrap();
    let human = vocabulary.resolve_label("Human").unwrap();
    let amount = if options.counted {
        InstructionAmount::ControlledPermanentsOfChosenType {
            chosen: CreatureTypeSlot::new(0),
        }
    } else {
        InstructionAmount::Literal(1.into())
    };
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
            amount: amount.clone(),
            recipients: TokenRecipients::EachPlayer,
        })
        .collect();
    let mut instructions = vec![];
    if options.counted {
        instructions.push(Instruction::ChooseCreatureType {
            into: CreatureTypeSlot::new(0),
            excluding: BTreeSet::new(),
        });
    }
    instructions.extend([
        Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(2.into()),
        },
        Instruction::CreateTokens { creations },
    ]);
    if options.tail_choice {
        instructions.push(Instruction::OptionalEffect {
            chooser: ResolvingPlayer::Controller,
        });
    }
    instructions.push(if options.ability {
        Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Gain(3.into()),
        }
    } else {
        Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(3.into()),
        }
    });
    let body = ExecutableBody::new(
        if options.ability {
            BodyTarget::SinglePlayer
        } else {
            BodyTarget::None
        },
        instructions,
    );
    let mut cards = vec![if options.ability {
        card(
            1,
            B,
            ObjectDestination::Battlefield { controller },
            Program::artifact(
                None,
                vec![ActivatedAbility {
                    id: ActivatedAbilityId(1),
                    mana_cost: ManaCost::free(),
                    source_cost: SourceCost::None,
                    body,
                }],
            ),
        )
    } else {
        card(
            1,
            controller,
            ObjectDestination::Hand,
            Program::instant(ManaCost::free(), body),
        )
    }];
    if options.counted {
        let mut definition = creature();
        definition.creature_subtypes.insert(human);
        cards.extend([
            card(
                50,
                B,
                ObjectDestination::Battlefield { controller },
                definition.clone(),
            ),
            card(
                51,
                controller,
                ObjectDestination::Battlefield { controller },
                definition,
            ),
        ]);
    }
    if options.lower {
        cards.push(card(
            2,
            D,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(5.into()),
                    }],
                ),
            ),
        ));
    }
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    if options.lower {
        for actor in [A, B, C] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
        state = apply(
            &state,
            D,
            GameCommand::CastUntargeted {
                spell: object(&state, 2),
                payment: None,
            },
        )
        .into_state();
        state = apply(&state, D, GameCommand::Pass).into_state();
    }
    while holder(&state) != controller {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    let command = if options.ability {
        GameCommand::ActivateAbility {
            source: object(&state, 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(controller),
            payment: None,
        }
    } else {
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        }
    };
    state = apply(&state, controller, command).into_state();
    for _ in 0..4 {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    if options.counted {
        state = reply(&state, DecisionReply::CreatureType(human)).into_state();
    }
    assert_eq!(state.execution().life_total(controller), Some(&22.into()));
    assert_no_birth(&state);
    state
}
fn reply(state: &GameState, choice: DecisionReply) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    apply(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice,
        },
    )
}
fn frame(state: &GameState) -> &TokenTimestampOrder {
    let RuleFrame::TimestampOrder(frame) = state.pending_decision().unwrap().frame() else {
        panic!("timestamp frame")
    };
    frame
}
fn reverse(state: &GameState) -> NativeCandidate {
    reply(
        state,
        DecisionReply::TimestampOrder(
            frame(state)
                .members()
                .iter()
                .rev()
                .map(|member| member.member)
                .collect(),
        ),
    )
}
fn ordinals(state: &GameState) -> Vec<usize> {
    frame(state)
        .members()
        .iter()
        .map(|member| member.member.ordinal())
        .collect()
}
fn assert_no_birth(state: &GameState) {
    assert!(
        state
            .execution()
            .window()
            .cards()
            .iter()
            .all(|object| !object.is_token())
    );
}
fn finish(mut state: GameState) -> GameState {
    for _ in 0..10 {
        match state.pending_decision().map(|pending| pending.frame()) {
            Some(RuleFrame::TimestampOrder(_)) => state = reverse(&state).into_state(),
            Some(RuleFrame::OptionalEffect(_)) => {
                state = reply(
                    &state,
                    DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
                )
                .into_state()
            }
            None => return state,
            _ => panic!("unexpected owner"),
        }
    }
    panic!("bounded order finish")
}
fn token_order(state: &GameState) -> Vec<(SeatId, String)> {
    let timestamps = state
        .execution()
        .characteristic_timestamps()
        .map(|(id, value)| (id, value.sequence()))
        .collect::<BTreeMap<_, _>>();
    let mut tokens = state
        .execution()
        .window()
        .cards()
        .iter()
        .filter(|object| object.is_token())
        .collect::<Vec<_>>();
    tokens.sort_by_key(|object| timestamps[&object.id()]);
    tokens
        .into_iter()
        .map(|object| {
            (
                object.owner(),
                String::from(
                    state
                        .execution()
                        .power_toughness(object.id())
                        .unwrap()
                        .unwrap()
                        .derived
                        .power,
                ),
            )
        })
        .collect()
}
fn expected(owners: &[SeatId], count: usize) -> Vec<(SeatId, String)> {
    owners
        .iter()
        .flat_map(|owner| {
            [2, 1]
                .into_iter()
                .flat_map(move |power| std::iter::repeat_n((*owner, power.to_string()), count))
        })
        .collect()
}
#[test]
fn current_creator_departure_keeps_accepted_prefix_and_sparse_member_birth_mapping() {
    let state = prepare(Fixture::default());
    assert_eq!(ordinals(&state), vec![0, 4]);
    let state = reverse(&state).into_state();
    assert_eq!(ordinals(&state), vec![1, 5]);
    assert_no_birth(&state);
    let next = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(frame(&next).chooser(), C);
    assert_eq!(ordinals(&next), vec![2, 6]);
    assert_no_birth(&next);
    let next = reverse(&next).into_state();
    assert_eq!(ordinals(&next), vec![3, 7]);
    assert_no_birth(&next);
    let born = reverse(&next);
    let done = born.state();
    assert!(done.pending_decision().is_none());
    assert_eq!(done.execution().life_total(A), Some(&25.into()));
    assert_eq!(token_order(done), expected(&[A, C, D], 1));
    let rows = born
        .rules_journal()
        .records
        .iter()
        .flat_map(|record| match record {
            RulesRecord::Event(event) => event
                .occurrences
                .iter()
                .filter_map(|value| {
                    if let RulesOccurrence::TokenCreated { creator, after } = &value.value {
                        Some((
                            *creator,
                            String::from(
                                done.execution()
                                    .power_toughness(after.object.id())
                                    .unwrap()
                                    .unwrap()
                                    .derived
                                    .power,
                            ),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![
            (A, "1".into()),
            (C, "1".into()),
            (D, "1".into()),
            (A, "2".into()),
            (C, "2".into()),
            (D, "2".into())
        ]
    );
}
#[test]
fn earlier_accepted_and_future_creators_can_depart_without_restarting_survivor_order() {
    let state = prepare(Fixture {
        controller: Some(C),
        ..Fixture::default()
    });
    let state = reverse(&state).into_state();
    let next = apply(&state, A, GameCommand::Concede).into_state();
    assert_eq!(frame(&next).chooser(), B);
    assert_eq!(ordinals(&next), vec![1, 5]);
    assert_no_birth(&next);
    assert_eq!(next.execution().window().active_player(), None);
    let done = finish(next);
    assert_eq!(token_order(&done), expected(&[B, C, D], 1));
    assert_eq!(done.execution().life_total(C), Some(&25.into()));
    let state = prepare(Fixture::default());
    let state = reverse(&state).into_state();
    let next = apply(&state, D, GameCommand::Concede).into_state();
    assert_eq!(frame(&next).chooser(), B);
    assert_eq!(ordinals(&next), vec![1, 5]);
    assert_no_birth(&next);
    let done = finish(next);
    assert_eq!(token_order(&done), expected(&[A, B, C], 1));
}
#[test]
fn departing_owned_source_does_not_cancel_survivor_token_instruction() {
    let state = prepare(Fixture {
        ability: true,
        ..Fixture::default()
    });
    let saved = state.execution().resolution_state();
    let next = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(next.execution().resolution_state(), saved);
    assert!(
        next.execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .is_none()
    );
    assert_eq!(ordinals(&next), vec![0, 4]);
    assert_no_birth(&next);
    let done = finish(next);
    assert_eq!(token_order(&done), expected(&[A, C, D], 1));
    assert_eq!(done.execution().life_total(A), Some(&25.into()));
}
#[test]
fn frozen_counts_and_nonempty_locals_survive_contributor_and_source_departure() {
    let state = prepare(Fixture {
        ability: true,
        counted: true,
        ..Fixture::default()
    });
    assert_eq!(frame(&state).members().len(), 4);
    let state = reverse(&state).into_state();
    let saved = state.execution().resolution_state().unwrap();
    assert_eq!(saved.locals().len(), 1);
    let next = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(next.execution().resolution_state(), Some(saved));
    assert!(
        next.execution()
            .window()
            .cards()
            .current_identity(CardId(50))
            .is_none()
    );
    assert_eq!(frame(&next).members().len(), 4);
    let done = finish(next);
    assert_eq!(token_order(&done), expected(&[A, C, D], 2));
    assert_eq!(done.execution().life_total(A), Some(&25.into()));
}
#[test]
fn controller_departure_cancels_unborn_batch_and_lower_spell_still_resolves() {
    let state = prepare(Fixture {
        ability: true,
        lower: true,
        ..Fixture::default()
    });
    let state = reverse(&state).into_state();
    let mut next = apply(&state, A, GameCommand::Concede).into_state();
    assert!(next.pending_decision().is_none());
    assert_no_birth(&next);
    assert_eq!(next.execution().window().entries().len(), 1);
    for _ in 0..3 {
        next = apply(&next, holder(&next), GameCommand::Pass).into_state();
    }
    assert!(next.execution().window().entries().is_empty());
    assert_no_birth(&next);
    assert_eq!(next.execution().life_total(D), Some(&25.into()));
    assert_eq!(next.execution().life_total(A), Some(&22.into()));
}
#[test]
fn stale_member_frames_and_late_birth_timestamp_decision_failures_publish_nothing() {
    use crate::effects::private_qualification::QualificationFault;
    let state = prepare(Fixture {
        tail_choice: true,
        ..Fixture::default()
    });
    let state = reverse(&state).into_state();
    let old = frame(&state).clone();
    assert_eq!(
        state
            .execution()
            .depart_players(&BTreeSet::from([B]), state.turn.as_ref(), None),
        Err(EffectError::UnsupportedDepartureState)
    );
    let next = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(
        next.execution().resume_rule(
            next.execution().revision(),
            &crate::effects::EffectDecision::TimestampOrder(old.clone()),
            C,
            DecisionReply::TimestampOrder(
                old.members().iter().map(|member| member.member).collect()
            ),
            next.turn.as_ref()
        ),
        Err(EffectError::WrongTimestampOrderFrame)
    );
    let before = next.clone();
    assert!(
        next.advance(input(
            &next,
            C,
            GameCommand::Reply {
                decision: next.pending_decision().unwrap().id(),
                choice: DecisionReply::TimestampOrder(vec![
                    TokenBatchMember::new(1),
                    TokenBatchMember::new(5)
                ])
            }
        ))
        .is_err()
    );
    assert_eq!(next, before);
    for fault in 0..3 {
        let mut bad = state.clone();
        match fault {
            0 => bad.set_test_input_revision(u64::MAX),
            1 => bad.set_test_check_effect_fault(QualificationFault::Revision(u64::MAX)),
            2 => bad.set_test_next_decision(u64::MAX),
            _ => unreachable!(),
        }
        let before = bad.clone();
        assert!(bad.advance(input(&bad, B, GameCommand::Concede)).is_err());
        assert_eq!(bad, before);
    }
    let last = reverse(&next).into_state();
    assert_eq!(frame(&last).chooser(), D);
    assert_no_birth(&last);
    let born = apply(&last, D, GameCommand::Concede);
    assert!(matches!(
        born.state().pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    assert_eq!(token_order(born.state()), expected(&[A, C], 1));
    assert_eq!(born.state().execution().life_total(A), Some(&22.into()));
    for fault in 0..4 {
        let mut bad = last.clone();
        match fault {
            0 => bad.set_test_check_effect_fault(QualificationFault::ObjectIdentity(u64::MAX)),
            1 => bad.set_test_check_effect_fault(QualificationFault::ObjectRevision(u64::MAX)),
            2 => bad
                .set_test_check_effect_fault(QualificationFault::CharacteristicTimestamp(u64::MAX)),
            3 => bad.set_test_next_decision(u64::MAX),
            _ => unreachable!(),
        }
        let before = bad.clone();
        let issued = input(&bad, D, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert!(result.is_err(), "late fault {fault}");
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, before);
        assert_no_birth(&bad);
    }
}
