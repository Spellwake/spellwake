//! Paid mana-color child and its exact Priority/Payment return ownership.
use super::*;
use crate::effects::{ManaColorChoice, ResolutionPaymentOutcome, ResolutionPaymentReply};
use crate::mana::{ManaPayment, ManaSpend};
const D: SeatId = SeatId(77);
fn offered(payment: bool, self_payer: bool) -> GameState {
    offered_with_lower(payment, self_payer, false)
}
fn offered_with_lower(payment: bool, self_payer: bool, lower: bool) -> GameState {
    let chooser = if payment && !self_payer { B } else { A };
    let land = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::ChooseColor {
                amount: 1.into(),
                colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
            },
        }],
    );
    let mut cards = vec![card(
        70,
        C,
        ObjectDestination::Battlefield {
            controller: chooser,
        },
        land,
    )];
    if payment {
        cards.push(card(
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
                        Instruction::OfferManaPayment {
                            payer: ResolvingPlayer::TargetPlayer,
                            cost: ManaCost::one(ManaType::Red),
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
                ),
            ),
        ));
    }
    if lower {
        cards.push(card(
            71,
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
        cards.push(card(
            72,
            D,
            ObjectDestination::Battlefield { controller: B },
            Program::land(
                BTreeSet::new(),
                vec![ManaAbility {
                    id: ManaAbilityId(8),
                    output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Green, 1.into())])),
                }],
            ),
        ));
    }
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    if lower {
        for actor in [A, B, C] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
        state = apply(
            &state,
            D,
            GameCommand::CastUntargeted {
                spell: object(&state, 71),
                payment: None,
            },
        )
        .into_state();
        state = apply(&state, D, GameCommand::Pass).into_state();
    }
    if payment {
        state = apply(
            &state,
            A,
            GameCommand::Cast {
                spell: object(&state, 1),
                target: TargetRef::Player(chooser),
            },
        )
        .into_state();
        for actor in [A, B, C, D] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
    }
    if lower {
        state = apply(
            &state,
            B,
            GameCommand::ActivateMana {
                source: object(&state, 72),
                ability: ManaAbilityRef::Printed(ManaAbilityId(8)),
            },
        )
        .into_state();
        assert_eq!(state.execution().mana(B, ManaType::Green), 1.into());
    }
    state = apply(
        &state,
        chooser,
        GameCommand::ActivateMana {
            source: object(&state, 70),
            ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    )
    .into_state();
    assert!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 70))
            .unwrap()
            .tapped()
            .unwrap()
    );
    assert_eq!(state.execution().mana(chooser, ManaType::Red), 0.into());
    state
}
fn frame(state: &GameState) -> &ManaColorChoice {
    let RuleFrame::ManaColor(frame) = state.pending_decision().unwrap().frame() else {
        panic!("color child")
    };
    frame
}
fn color(state: &GameState) -> NativeCandidate {
    apply(
        state,
        frame(state).chooser(),
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::ManaColor(ManaType::Red),
        },
    )
}
fn no_production(candidate: &NativeCandidate) {
    assert!(!candidate.rules_journal().records.iter().any(|record| matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(value.value,RulesOccurrence::ManaProduced {..})))));
}
fn retained(before: &GameState, after: &GameState) {
    let a = frame(before);
    let b = frame(after);
    assert_eq!(
        (a.source(), a.ability(), a.chooser(), a.amount(), a.colors()),
        (b.source(), b.ability(), b.chooser(), b.amount(), b.colors())
    );
    assert_ne!(a.revision(), b.revision());
    assert_ne!(
        before.pending_decision().unwrap().id(),
        after.pending_decision().unwrap().id()
    );
    assert_eq!(
        after.execution().resume_mana_color(
            after.execution().revision(),
            a,
            a.chooser(),
            ManaType::Red
        ),
        Err(EffectError::WrongManaColorFrame)
    );
}
#[test]
fn priority_color_survives_source_owner_departure_and_produces_once_without_reactivation() {
    let state = offered(false, false);
    let candidate = apply(&state, C, GameCommand::Concede);
    no_production(&candidate);
    let next = candidate.into_state();
    retained(&state, &next);
    let repeated = apply(&next, D, GameCommand::Concede);
    no_production(&repeated);
    let repeated = repeated.into_state();
    retained(&next, &repeated);
    let next = repeated;
    assert!(
        next.execution()
            .window()
            .cards()
            .current_identity(CardId(70))
            .is_none()
    );
    let done = color(&next);
    assert_eq!(done.state().execution().mana(A, ManaType::Red), 1.into());
    assert!(done.state().pending_decision().is_none());
    assert_eq!(holder(done.state()), A);
    assert_eq!(done.rules_journal().records.iter().filter(|record| matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(value.value,RulesOccurrence::ManaProduced {..})))).count(),1);
}
#[test]
fn departed_priority_chooser_retires_child_without_mana_and_next_survivor_gets_priority() {
    let state = offered(false, false);
    let candidate = apply(&state, A, GameCommand::Concede);
    no_production(&candidate);
    let next = candidate.state();
    assert!(next.pending_decision().is_none());
    assert!(next.execution().pending_mana_color().is_none());
    assert_eq!(holder(next), B);
    assert_eq!(next.execution().window().active_player(), None);
}
#[test]
fn payment_color_survives_gone_source_then_restores_exact_parent_and_pays_once() {
    let state = offered(true, false);
    let before = state.execution().resolution_choice_state().unwrap();
    let candidate = apply(&state, C, GameCommand::Concede);
    no_production(&candidate);
    let next = candidate.into_state();
    retained(&state, &next);
    assert_eq!(next.execution().resolution_choice_state(), Some(before));
    let ready = color(&next).into_state();
    assert_eq!(ready.execution().mana(B, ManaType::Red), 1.into());
    let decision = ready.pending_decision().unwrap();
    assert_eq!(decision.chooser(), B);
    assert!(matches!(decision.frame(), RuleFrame::ResolutionPayment(_)));
    let paid = apply(
        &ready,
        B,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Pay(ManaPayment {
                spend: vec![ManaSpend {
                    mana_type: ManaType::Red,
                    amount: 1.into(),
                }],
            })),
        },
    );
    assert_eq!(paid.state().execution().mana(B, ManaType::Red), 0.into());
    assert_eq!(paid.state().execution().life_total(A), Some(&29.into()));
}
#[test]
fn departed_payment_color_chooser_runs_saved_unpaid_branch_without_production() {
    let state = offered(true, false);
    let candidate = apply(&state, B, GameCommand::Concede);
    no_production(&candidate);
    assert_eq!(
        candidate.state().execution().life_total(A),
        Some(&25.into())
    );
    assert!(candidate.state().pending_decision().is_none());
    assert_eq!(
        candidate
            .rules_journal()
            .records
            .iter()
            .filter(|row| matches!(
                row,
                RulesRecord::ResolutionPaymentOutcome {
                    outcome: ResolutionPaymentOutcome::UnpaidDeparted,
                    ..
                }
            ))
            .count(),
        1
    );
}
#[test]
fn payment_entry_departure_preserves_independent_live_color_child_with_priority_return() {
    let state = offered_with_lower(true, false, true);
    let candidate = apply(&state, A, GameCommand::Concede);
    no_production(&candidate);
    let next = candidate.into_state();
    retained(&state, &next);
    assert_eq!(next.execution().window().entries().len(), 1);
    assert_eq!(next.execution().mana(B, ManaType::Green), 1.into());
    for id in [70, 72] {
        assert_eq!(
            next.execution()
                .window()
                .cards()
                .get(object(&next, id))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    assert!(next.execution().resolution_choice_state().is_none());
    assert_eq!(next.execution().window().active_player(), None);
    let retired = apply(&next, B, GameCommand::Concede);
    no_production(&retired);
    assert!(retired.state().pending_decision().is_none());
    assert_eq!(holder(retired.state()), C);
    assert!(
        !retired
            .rules_journal()
            .records
            .iter()
            .any(|row| matches!(row, RulesRecord::ResolutionPaymentOutcome { .. }))
    );
    let done = color(&next);
    assert_eq!(done.state().execution().mana(B, ManaType::Red), 1.into());
    assert_eq!(done.state().execution().mana(B, ManaType::Green), 1.into());
    for id in [70, 72] {
        assert_eq!(
            done.state()
                .execution()
                .window()
                .cards()
                .get(object(done.state(), id))
                .unwrap()
                .tapped(),
            Some(true)
        );
    }
    assert_eq!(holder(done.state()), B);
    assert!(done.state().pending_decision().is_none());
    assert_eq!(done.state().execution().life_total(A), Some(&22.into()));
    assert!(
        !done
            .rules_journal()
            .records
            .iter()
            .any(|row| matches!(row, RulesRecord::ResolutionPaymentOutcome { .. }))
    );
    let mut continued = done.into_state();
    for _ in 0..3 {
        continued = apply(&continued, holder(&continued), GameCommand::Pass).into_state();
    }
    assert!(continued.execution().window().entries().is_empty());
    assert_eq!(continued.execution().life_total(D), Some(&25.into()));
    assert_eq!(continued.execution().mana(B, ManaType::Red), 1.into());
    assert_eq!(continued.execution().mana(B, ManaType::Green), 1.into());
}
#[test]
fn same_departed_parent_and_child_retire_without_outcome_and_late_failures_remain_atomic() {
    use crate::effects::private_qualification::QualificationFault;
    let state = offered(true, true);
    let candidate = apply(&state, A, GameCommand::Concede);
    no_production(&candidate);
    assert!(candidate.state().pending_decision().is_none());
    assert!(candidate.state().execution().window().entries().is_empty());
    assert!(
        !candidate
            .rules_journal()
            .records
            .iter()
            .any(|row| matches!(row, RulesRecord::ResolutionPaymentOutcome { .. }))
    );
    for fault in 0..3 {
        let mut state = offered(true, false);
        match fault {
            0 => state.set_test_input_revision(u64::MAX),
            1 => state.set_test_check_effect_fault(QualificationFault::Revision(u64::MAX)),
            2 => state.set_test_next_decision(u64::MAX),
            _ => unreachable!(),
        }
        let before = state.clone();
        let issued = input(&state, C, GameCommand::Concede);
        let result = state.advance(issued.clone());
        assert!(result.is_err());
        assert_eq!(result, state.advance(issued));
        assert_eq!(state, before);
    }
}

#[test]
fn ordinary_resolution_choice_keeps_issued_frame_after_unrelated_departure() {
    let program = Program::instant(
        ManaCost::free(),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::ChooseCreatureType {
                into: CreatureTypeSlot::new(0),
                excluding: BTreeSet::new(),
            }],
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
        RuleFrame::CreatureType(_)
    ));
    let next = apply(&state, C, GameCommand::Concede).into_state();
    assert!(matches!(
        next.pending_decision().unwrap().frame(),
        RuleFrame::CreatureType(_)
    ));
    assert_eq!(next.pending_decision().unwrap().chooser(), A);
}
