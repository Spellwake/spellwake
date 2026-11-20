//! Real suspended optional owners survive or cease without replaying prefixes.
use super::*;
use crate::effects::{OptionalEffectChoice, ResolutionChoiceState};
use crate::stack_window::StackEntryId;
const D: SeatId = SeatId(77);
const E: SeatId = SeatId(88);

fn pending(targeted_effect: bool, lower: bool) -> GameState {
    let mut cards = vec![card(
        1,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePlayer,
                vec![
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(2.into()),
                    },
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::TargetPlayer,
                    },
                    if targeted_effect {
                        Instruction::AdjustTargetLife {
                            adjustment: LifeAdjustment::Gain(7.into()),
                        }
                    } else {
                        Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(7.into()),
                        }
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    },
                ],
            ),
        ),
    )];
    if lower {
        cards.push(card(
            2,
            C,
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
    let mut state = root(
        cards,
        &[20; 5],
        Some(Step::PrecombatMain),
        vec![A, B, C, D, E],
    );
    if lower {
        for actor in [A, B] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
        state = apply(
            &state,
            C,
            GameCommand::CastUntargeted {
                spell: object(&state, 2),
                payment: None,
            },
        )
        .into_state();
        for actor in [C, D, E] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
    }
    state = apply(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Player(B),
        },
    )
    .into_state();
    for actor in [A, B, C, D, E] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::OptionalEffect(_)
    ));
    state
}
fn answer(state: &GameState, choice: DecisionReply) -> GameState {
    let decision = state.pending_decision().unwrap();
    apply(
        state,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
    )
    .into_state()
}
fn cursor(state: &GameState) -> (StackEntryId, usize) {
    match state.execution().resolution_choice_state().unwrap() {
        ResolutionChoiceState::Optional { cursor, .. }
        | ResolutionChoiceState::ChooserSubstitution { cursor, .. } => {
            (cursor.entry(), cursor.instruction())
        }
        _ => panic!("optional owner"),
    }
}
#[test]
fn unrelated_departure_keeps_effective_chooser_cursor_and_single_prefix() {
    let state = pending(false, false);
    let original = cursor(&state);
    let old = input(
        &state,
        B,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        },
    );
    let next = apply(&state, E, GameCommand::Concede).into_state();
    assert_eq!(cursor(&next), original);
    assert_eq!(next.pending_decision().unwrap().chooser(), B);
    assert_eq!(next.execution().life_total(A), Some(&22.into()));
    assert_eq!(next.advance(old.clone()), Err(GameError::StaleRevision));
    let mut wrong = old;
    wrong.basis = next.basis().clone();
    assert_eq!(next.advance(wrong), Err(GameError::WrongDecision));
    for (choice, total) in [
        (OptionalEffectChoice::Take, 32),
        (OptionalEffectChoice::Decline, 25),
    ] {
        let done = answer(&next, DecisionReply::OptionalEffect(choice));
        assert_eq!(done.execution().life_total(A), Some(&total.into()));
        assert!(done.pending_decision().is_none());
    }
}
#[test]
fn repeated_selected_chooser_departures_and_candidate_departure_keep_one_cursor() {
    let state = pending(false, false);
    let original = cursor(&state);
    let mut state = apply(&state, B, GameCommand::Concede).into_state();
    let RuleFrame::ReplacementChooser(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.candidates(), &[C, D, E]);
    assert_eq!(frame.departed(), B);
    // Candidate leaves while the controller has not yet answered.
    state = apply(&state, C, GameCommand::Concede).into_state();
    let RuleFrame::ReplacementChooser(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.candidates(), &[D, E]);
    assert_eq!(frame.departed(), B);
    state = answer(&state, DecisionReply::ReplacementChooser(D));
    assert_eq!(state.pending_decision().unwrap().chooser(), D);
    state = apply(&state, D, GameCommand::Concede).into_state();
    let RuleFrame::ReplacementChooser(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.candidates(), &[E]);
    assert_eq!(frame.departed(), D);
    assert_eq!(cursor(&state), original);
    state = answer(&state, DecisionReply::ReplacementChooser(E));
    state = answer(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(state.execution().life_total(A), Some(&32.into()));
    assert_eq!(state.execution().window().participants(), &[A, E]);
}
#[test]
fn vanished_target_removes_take_and_never_mutates_departed_life_lki() {
    let state = pending(true, false);
    let state = apply(&state, B, GameCommand::Concede).into_state();
    let state = answer(&state, DecisionReply::ReplacementChooser(D));
    let RuleFrame::OptionalEffect(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(
        frame.options(),
        &BTreeSet::from([OptionalEffectChoice::Decline])
    );
    assert_eq!(
        state.advance(input(
            &state,
            D,
            GameCommand::Reply {
                decision: state.pending_decision().unwrap().id(),
                choice: DecisionReply::OptionalEffect(OptionalEffectChoice::Take)
            }
        )),
        Err(GameError::Effect(EffectError::IllegalOptionalEffect))
    );
    let state = answer(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(state.execution().life_total(B), Some(&20.into()));
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
}
#[test]
fn departed_resolving_controller_ceases_only_its_work_and_lower_stack_continues() {
    let state = pending(false, true);
    let candidate = apply(&state, A, GameCommand::Concede);
    let RulesRecord::PlayersDeparted { ceased_entries, .. } = &candidate.rules_journal().records[0]
    else {
        panic!()
    };
    assert_eq!(ceased_entries.len(), 1);
    assert!(
        !candidate
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(record, RulesRecord::Event(event) if matches!(event.cause, crate::occurrence::EventCause::ResolutionCompleted { .. })))
    );
    let mut state = candidate.into_state();
    assert!(state.pending_decision().is_none());
    assert!(state.execution().pending_work_kind().is_none());
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    assert_eq!(state.execution().window().active_player(), None);
    assert_eq!(state.execution().window().entries().len(), 1);
    for _ in 0..4 {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().life_total(C), Some(&25.into()));
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
}
#[test]
fn pending_departure_rejects_allocator_exhaustion_without_adopting_prefix() {
    use crate::effects::private_qualification::QualificationFault;
    for fault in 0..3 {
        let mut state = pending(false, false);
        match fault {
            0 => state.set_test_input_revision(u64::MAX),
            1 => state.set_test_check_effect_fault(QualificationFault::Revision(u64::MAX)),
            2 => state.set_test_next_decision(u64::MAX),
            _ => unreachable!(),
        }
        let before = state.clone();
        let issued = input(&state, B, GameCommand::Concede);
        let result = state.advance(issued.clone());
        assert!(result.is_err());
        assert_eq!(result, state.advance(issued));
        assert_eq!(state, before);
    }
}

#[test]
fn replacement_can_itself_leave_twice_without_reverting_to_original_binding() {
    let state = pending(false, false);
    let original = cursor(&state);
    let mut state = apply(&state, B, GameCommand::Concede).into_state();
    for (selected, next) in [(D, E), (E, C)] {
        state = answer(&state, DecisionReply::ReplacementChooser(selected));
        state = apply(&state, selected, GameCommand::Concede).into_state();
        let RuleFrame::ReplacementChooser(frame) = state.pending_decision().unwrap().frame() else {
            panic!()
        };
        assert_eq!(frame.departed(), selected);
        assert!(frame.candidates().contains(&next));
        assert_eq!(cursor(&state), original);
    }
    state = answer(&state, DecisionReply::ReplacementChooser(C));
    state = answer(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
    );
    assert_eq!(state.execution().life_total(A), Some(&32.into()));
}
#[test]
fn departed_owned_copy_receiver_disables_take_but_survivor_controlled_ability_continues() {
    use crate::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
    let receiver = Program::artifact(
        None,
        vec![ActivatedAbility {
            id: ActivatedAbilityId(1),
            mana_cost: ManaCost::free(),
            source_cost: SourceCost::None,
            body: ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![
                    Instruction::OptionalEffect {
                        chooser: ResolvingPlayer::Controller,
                    },
                    Instruction::CopyTargetToSource {
                        duration: ResolvedDuration::NoStatedDuration,
                        power_toughness: None,
                    },
                    Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(3.into()),
                    },
                ],
            ),
        }],
    );
    let mut state = root(
        vec![
            card(
                1,
                B,
                ObjectDestination::Battlefield { controller: A },
                receiver,
            ),
            card(
                2,
                C,
                ObjectDestination::Battlefield { controller: C },
                creature(),
            ),
        ],
        &[20; 3],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    state = apply(
        &state,
        A,
        GameCommand::ActivateAbility {
            source: object(&state, 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Object(object(&state, 2)),
            payment: None,
        },
    )
    .into_state();
    for actor in [A, B, C] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    let original = cursor(&state);
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(cursor(&state), original);
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .is_none()
    );
    let RuleFrame::OptionalEffect(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(
        frame.options(),
        &BTreeSet::from([OptionalEffectChoice::Decline])
    );
    let state = answer(
        &state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
    );
    assert_eq!(state.execution().life_total(A), Some(&23.into()));
}
#[test]
fn unrelated_pending_payment_owner_preserves_quote_during_departure() {
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![
                Instruction::OfferManaPayment {
                    payer: ResolvingPlayer::Controller,
                    cost: ManaCost::free(),
                    paid_destination: 1,
                    declined_destination: 1,
                },
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(3.into()),
                },
            ],
        ),
    );
    let mut state = root(
        vec![card(1, A, ObjectDestination::Hand, program)],
        &[20; 3],
        Some(Step::PrecombatMain),
        vec![A, B, C],
    );
    state = apply(
        &state,
        A,
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        },
    )
    .into_state();
    for actor in [A, B, C] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    let next = apply(&state, C, GameCommand::Concede).into_state();
    assert!(matches!(
        next.pending_decision().unwrap().frame(),
        RuleFrame::ResolutionPayment(_)
    ));
    assert_eq!(next.pending_decision().unwrap().chooser(), A);
}
