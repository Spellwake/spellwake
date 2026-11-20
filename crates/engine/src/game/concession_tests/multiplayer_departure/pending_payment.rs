//! CR800.4f completes a saved payment owner without a departed player's reply.
use super::*;
use crate::effects::{OptionalEffectChoice, ResolutionPaymentOutcome, ResolutionPaymentReply};
use crate::mana::{ManaPayment, ManaSpend};
const D: SeatId = SeatId(77);
fn offered(cost: ManaCost, color: bool, wait_after_unpaid: bool) -> GameState {
    offered_with_lower(cost, color, wait_after_unpaid, false)
}
fn offered_with_lower(
    cost: ManaCost,
    color: bool,
    wait_after_unpaid: bool,
    lower: bool,
) -> GameState {
    let tail = if wait_after_unpaid {
        vec![
            Instruction::OptionalEffect {
                chooser: ResolvingPlayer::Controller,
            },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(3.into()),
            },
        ]
    } else {
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(3.into()),
        }]
    };
    let mut body = vec![
        Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(2.into()),
        },
        Instruction::OfferManaPayment {
            payer: ResolvingPlayer::TargetPlayer,
            cost,
            paid_destination: 2,
            declined_destination: 4,
        },
        Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(7.into()),
        },
        Instruction::Jump {
            destination: 4 + tail.len(),
        },
    ];
    body.extend(tail);
    let land = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: if color {
                ManaOutput::ChooseColor {
                    amount: 1.into(),
                    colors: BTreeSet::from([ManaType::Red, ManaType::Green]),
                }
            } else {
                ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 1.into())]))
            },
        }],
    );
    let mut cards = vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(BodyTarget::SinglePlayer, body),
            ),
        ),
        card(
            70,
            C,
            ObjectDestination::Battlefield { controller: B },
            land,
        ),
    ];
    if lower {
        cards.push(card(
            71,
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
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    if lower {
        for actor in [A, B] {
            state = apply(&state, actor, GameCommand::Pass).into_state();
        }
        state = apply(
            &state,
            C,
            GameCommand::CastUntargeted {
                spell: object(&state, 71),
                payment: None,
            },
        )
        .into_state();
        for actor in [C, D] {
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
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    state
}
fn respond(state: &GameState, reply: ResolutionPaymentReply) -> NativeCandidate {
    apply(
        state,
        B,
        GameCommand::Reply {
            decision: state.pending_decision().unwrap().id(),
            choice: DecisionReply::ResolutionPayment(reply),
        },
    )
}
#[test]
fn departed_payer_uses_exact_authored_unpaid_branch_for_free_and_nonzero_cost() {
    for cost in [ManaCost::free(), ManaCost::one(ManaType::Red)] {
        let state = offered(cost.clone(), false, false);
        let RuleFrame::ResolutionPayment(frame) = state.pending_decision().unwrap().frame() else {
            panic!()
        };
        let entry = frame.entry();
        let candidate = apply(&state, B, GameCommand::Concede);
        assert_eq!(
            candidate.state().execution().life_total(A),
            Some(&25.into())
        );
        assert!(candidate.state().pending_decision().is_none());
        assert!(candidate.state().execution().window().entries().is_empty());
        let outcomes = candidate
            .rules_journal()
            .records
            .iter()
            .filter(|record| matches!(record, RulesRecord::ResolutionPaymentOutcome { .. }))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes,
            &[&RulesRecord::ResolutionPaymentOutcome {
                entry,
                instruction: 1,
                payer: B,
                cost,
                outcome: ResolutionPaymentOutcome::UnpaidDeparted,
                next: 4
            }]
        );
        assert!(!candidate.rules_journal().records.iter().any(|record| matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|occurrence| matches!(occurrence.value,RulesOccurrence::ManaDebited {..})))));
    }
}
#[test]
fn survivor_payer_keeps_quote_and_credited_mana_after_source_owner_leaves() {
    let state = offered(ManaCost::one(ManaType::Red), false, false);
    let state = apply(
        &state,
        B,
        GameCommand::ActivateMana {
            source: object(&state, 70),
            ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    )
    .into_state();
    assert_eq!(state.execution().mana(B, ManaType::Red), 1.into());
    let old = state.pending_decision().unwrap();
    let RuleFrame::ResolutionPayment(before) = old.frame() else {
        panic!()
    };
    let next = apply(&state, C, GameCommand::Concede).into_state();
    assert!(
        next.execution()
            .window()
            .cards()
            .current_identity(CardId(70))
            .is_none()
    );
    assert_eq!(next.execution().mana(B, ManaType::Red), 1.into());
    let RuleFrame::ResolutionPayment(after) = next.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(
        (
            before.entry(),
            before.instruction(),
            before.payer(),
            before.cost()
        ),
        (
            after.entry(),
            after.instruction(),
            after.payer(),
            after.cost()
        )
    );
    assert_ne!(old.id(), next.pending_decision().unwrap().id());
    assert_eq!(
        next.advance(input(
            &next,
            B,
            GameCommand::Reply {
                decision: old.id(),
                choice: DecisionReply::ResolutionPayment(ResolutionPaymentReply::Decline)
            }
        )),
        Err(GameError::WrongDecision)
    );
    let paid = respond(
        &next,
        ResolutionPaymentReply::Pay(ManaPayment {
            spend: vec![ManaSpend {
                mana_type: ManaType::Red,
                amount: 1.into(),
            }],
        }),
    );
    assert_eq!(paid.state().execution().mana(B, ManaType::Red), 0.into());
    assert_eq!(paid.state().execution().life_total(A), Some(&29.into()));
    let declined = respond(&next, ResolutionPaymentReply::Decline);
    assert_eq!(declined.state().execution().life_total(A), Some(&25.into()));
    assert_eq!(
        declined.state().execution().mana(B, ManaType::Red),
        1.into()
    );
}
#[test]
fn unpaid_progress_can_issue_next_choice_without_replaying_prefix_and_exhaustion_rolls_back() {
    use crate::effects::private_qualification::QualificationFault;
    let state = offered(ManaCost::free(), false, true);
    let next = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(next.execution().life_total(A), Some(&22.into()));
    let decision = next.pending_decision().unwrap();
    assert_eq!(decision.chooser(), A);
    let RuleFrame::OptionalEffect(frame) = decision.frame() else {
        panic!()
    };
    assert_eq!(frame.instruction(), 4);
    let done = apply(
        &next,
        A,
        GameCommand::Reply {
            decision: decision.id(),
            choice: DecisionReply::OptionalEffect(OptionalEffectChoice::Take),
        },
    );
    assert_eq!(done.state().execution().life_total(A), Some(&25.into()));
    for fault in 0..3 {
        let mut state = state.clone();
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
fn controller_departure_ceases_payment_without_outcome_and_nested_color_survives_source_loss() {
    let state = offered_with_lower(ManaCost::free(), false, false, true);
    let ceased = apply(&state, A, GameCommand::Concede);
    assert!(ceased.state().pending_decision().is_none());
    assert_eq!(ceased.state().execution().window().entries().len(), 1);
    assert_eq!(ceased.state().execution().life_total(A), Some(&22.into()));
    assert!(
        !ceased
            .rules_journal()
            .records
            .iter()
            .any(|row| matches!(row, RulesRecord::ResolutionPaymentOutcome { .. }))
    );
    let mut continued = ceased.into_state();
    for _ in 0..3 {
        continued = apply(&continued, holder(&continued), GameCommand::Pass).into_state();
    }
    assert!(continued.execution().window().entries().is_empty());
    assert_eq!(continued.execution().life_total(C), Some(&25.into()));
    let state = offered(ManaCost::one(ManaType::Red), true, false);
    let state = apply(
        &state,
        B,
        GameCommand::ActivateMana {
            source: object(&state, 70),
            ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    )
    .into_state();
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    let next = apply(&state, C, GameCommand::Concede).into_state();
    assert!(matches!(
        next.pending_decision().unwrap().frame(),
        RuleFrame::ManaColor(_)
    ));
    assert_eq!(next.pending_decision().unwrap().chooser(), B);
}
