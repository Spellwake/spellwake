//! Real priority Checks ordering; later waves retain separate placement ownership.
use super::*;
use crate::program::ObjectRelation;
const D: SeatId = SeatId(77);
fn prepare() -> GameState {
    prepare_with_exile(false)
}
fn prepare_with_exile(exile: bool) -> GameState {
    let mut cards = vec![
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    vec![Instruction::ReturnToOwnersHand],
                ),
            ),
        ),
        card(
            20,
            B,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
    ];
    for (id, owner, controller) in [
        (2, if exile { A } else { B }, A),
        (3, A, A),
        (4, C, C),
        (5, C, C),
        (6, D, D),
    ] {
        let program = watcher(
            id,
            OrdinaryCondition::LeavesBattlefield(ObjectRelation::AnyCreature),
        );
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller },
            program,
        ));
    }
    if exile {
        cards.push(card(
            99,
            C,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ));
    }
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    state = apply(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Object(object(&state, 20)),
        },
    )
    .into_state();
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Triggers(_)
    ));
    state
}
fn watcher(amount: u64, condition: OrdinaryCondition) -> Program {
    let mut program = Program::artifact(None, vec![]);
    program.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition,
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(i64::try_from(amount).unwrap().into()),
            }],
        ),
    });
    program
}
fn ordered(state: &GameState) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = pending.frame() else {
        panic!("actual trigger ordering")
    };
    let result = apply(
        state,
        frame.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OrderTriggers(frame.instances().iter().rev().copied().collect()),
        },
    );
    assert_eq!(
        placements(&result)
            .iter()
            .filter(|placement| placement.controller == frame.chooser()
                && placement.wave == frame.wave())
            .map(|placement| placement.instance)
            .collect::<Vec<_>>(),
        frame.instances().iter().rev().copied().collect::<Vec<_>>()
    );
    result
}
fn placements(candidate: &NativeCandidate) -> Vec<crate::occurrence::TriggerPlacement> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerPlaced(placement) => Some(placement.clone()),
            _ => None,
        })
        .collect()
}
fn assert_captured_payloads(
    state: &GameState,
    placements: &[crate::occurrence::TriggerPlacement],
    captures: &BTreeMap<crate::occurrence::TriggerInstanceId, crate::occurrence::WaitingInstance>,
) {
    for placement in placements {
        let capture = &captures[&placement.instance];
        assert_eq!(placement.controller, capture.controller);
        let payload = state
            .execution()
            .retained_stack_payloads()
            .find(|payload| payload.entry == placement.entry)
            .unwrap();
        let (body, binding) = capture.clone().into_payload_parts();
        assert_eq!(payload.body, Some(&body));
        assert_eq!(
            payload.bindings,
            Some(&crate::occurrence::ExecutionBindings {
                trigger: Some(binding)
            })
        );
        let entry = state
            .execution()
            .window()
            .entries()
            .iter()
            .find(|entry| entry.id() == placement.entry)
            .unwrap();
        assert!(
            matches!(entry.kind(),crate::stack_window::StackKind::TriggeredAbility {source} if *source==capture.source.object)
        );
    }
}

#[test]
fn surviving_trigger_capture_outlives_source_owner_and_departed_later_chooser_is_skipped() {
    let state = prepare();
    let original_captures = state
        .execution()
        .waiting_triggers()
        .map(|capture| (capture.id, capture.clone()))
        .collect::<BTreeMap<_, _>>();
    let old = state.pending_decision().unwrap().clone();
    let RuleFrame::Triggers(frame) = old.frame() else {
        panic!()
    };
    let wave = frame.wave();
    let instances = frame.instances().to_vec();
    let phase = state.execution().window().phase();
    let position = state.turn().unwrap().position();
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(2))
            .is_none()
    );
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.instances(), instances);
    assert_eq!(
        state
            .execution()
            .waiting_triggers()
            .map(|capture| (capture.id, capture.clone()))
            .collect::<BTreeMap<_, _>>(),
        original_captures
    );
    assert_eq!(frame.wave(), wave);
    assert_eq!(state.execution().window().phase(), phase);
    assert_eq!(state.turn().unwrap().position(), position);
    assert_ne!(state.pending_decision().unwrap().id(), old.id());
    assert_eq!(
        state.advance(input(
            &state,
            A,
            GameCommand::Reply {
                decision: old.id(),
                choice: DecisionReply::OrderTriggers(instances.clone())
            }
        )),
        Err(GameError::WrongDecision)
    );
    let placed_a = ordered(&state);
    assert_eq!(placements(&placed_a).len(), 2);
    let actual = placements(&placed_a);
    assert_eq!(
        actual.iter().map(|p| p.instance).collect::<Vec<_>>(),
        instances.iter().rev().copied().collect::<Vec<_>>()
    );
    assert_eq!(
        placed_a
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<Vec<_>>(),
        actual.iter().map(|p| p.entry).collect::<Vec<_>>()
    );
    assert_captured_payloads(placed_a.state(), &actual, &original_captures);
    let state = placed_a.state();
    assert_eq!(state.pending_decision().unwrap().chooser(), C);
    assert!(matches!(
        state.execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
    ));
    let departed = apply(state, C, GameCommand::Concede);
    let placed_d = placements(&departed);
    assert_eq!(placed_d.len(), 1);
    assert_eq!(placed_d[0].controller, D);
    assert_eq!(placed_d[0].wave, wave);
    assert!(departed.state().pending_decision().is_none());
    assert_eq!(departed.state().execution().window().entries().len(), 3);
    let mut state = departed.into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert_eq!(state.execution().life_total(C), Some(&20.into()));
    assert_eq!(state.execution().life_total(D), Some(&26.into()));
}
#[test]
fn departing_current_ordering_controller_loses_only_its_own_waiting_group() {
    let state = prepare();
    let departed = apply(&state, A, GameCommand::Concede);
    let RuleFrame::Triggers(frame) = departed.state().pending_decision().unwrap().frame() else {
        panic!("surviving C ordering")
    };
    assert_eq!(frame.chooser(), C);
    assert_eq!(frame.instances().len(), 2);
    assert!(placements(&departed).is_empty());
    let placed = ordered(departed.state());
    let records = placements(&placed);
    assert_eq!(
        records
            .iter()
            .map(|record| record.controller)
            .collect::<Vec<_>>(),
        vec![C, C, D]
    );
    assert!(placed.state().pending_decision().is_none());
    let mut state = placed.into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&20.into()));
    assert_eq!(state.execution().life_total(C), Some(&29.into()));
    assert_eq!(state.execution().life_total(D), Some(&26.into()));
    let mut absent_steps = 0;
    for _ in 0..40 {
        if state.turn().unwrap().position().number == 2 {
            break;
        }
        let candidate = apply(&state, holder(&state), GameCommand::Pass);
        for record in &candidate.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::StepBegan { position, active } = occurrence.value
                        && position.number == 1
                    {
                        assert_eq!(active, None);
                        absent_steps += 1;
                    }
                }
            }
        }
        state = candidate.into_state();
    }
    assert!(absent_steps > 0);
    assert_eq!(state.turn().unwrap().position().number, 2);
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(state.execution().window().active_player(), Some(B));
    assert_eq!(holder(&state), B);
}

#[test]
fn departure_discoveries_wait_for_a_distinct_later_wave_without_resetting_original_ordering() {
    let state = prepare_with_exile(true);
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    let initial_captures = state
        .execution()
        .waiting_triggers()
        .map(|waiting| (waiting.id, waiting.clone()))
        .collect::<BTreeMap<_, _>>();
    let exiled = object(&state, 99);
    let first_wave = frame.wave();
    let original = frame.instances().to_vec();
    let departed = apply(&state, B, GameCommand::Concede);
    let new_captures = departed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some((waiting.id, waiting.as_ref().clone())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(new_captures.len(), 5);
    let departure_event=departed.rules_journal().records.iter().find_map(|record|match record {RulesRecord::Event(event) if matches!(&event.cause,crate::occurrence::EventCause::PlayerDeparture {players} if *players==BTreeSet::from([B]))=>Some(event),_=>None}).unwrap();
    for capture in new_captures.values() {
        assert_eq!(capture.causes.event(), departure_event.id);
        assert_eq!(capture.causes.len(), 1);
        let member = &capture.causes.members()[0];
        assert_eq!(member.related_player, Some(B));
        assert!(departure_event.occurrences.iter().any(|occurrence|occurrence.id==member.occurrence && matches!(&occurrence.value,RulesOccurrence::ZoneChange(change) if change.before.object.id()==exiled && change.after.object.zone()==crate::object_identity::ObjectZone::Exile)));
    }
    assert!(
        initial_captures
            .values()
            .all(|capture| capture.causes.event() != departure_event.id)
    );
    let all_captures = initial_captures
        .iter()
        .chain(new_captures.iter())
        .map(|(id, capture)| (*id, capture.clone()))
        .collect::<BTreeMap<_, _>>();
    let state = departed.state();
    assert_eq!(
        state
            .execution()
            .waiting_triggers()
            .map(|waiting| (waiting.id, waiting.clone()))
            .collect::<BTreeMap<_, _>>(),
        all_captures
    );
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.instances(), original);
    assert_eq!(frame.wave(), first_wave);
    assert_eq!(state.execution().waiting_triggers().count(), 10);
    let first_a = ordered(state);
    assert_eq!(placements(&first_a).len(), 2);
    assert_eq!(first_a.state().pending_decision().unwrap().chooser(), C);
    let rest = ordered(first_a.state());
    assert_eq!(
        placements(&rest)
            .iter()
            .map(|p| p.controller)
            .collect::<Vec<_>>(),
        vec![C, C, D]
    );
    assert!(placements(&rest).iter().all(|p| p.wave == first_wave));
    let RuleFrame::Triggers(frame) = rest.state().pending_decision().unwrap().frame() else {
        panic!("new wave A order")
    };
    assert_eq!(frame.chooser(), A);
    assert_ne!(frame.wave(), first_wave);
    let second_wave = frame.wave();
    assert_eq!(frame.instances().len(), 2);
    assert!(matches!(
        rest.state().execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: A })
    ));
    let second_a = ordered(rest.state());
    let second_rest = ordered(second_a.state());
    assert!(
        placements(&second_a)
            .iter()
            .chain(placements(&second_rest).iter())
            .all(|p| p.wave == second_wave)
    );
    assert_eq!(second_rest.state().execution().window().entries().len(), 10);
    assert!(second_rest.state().pending_decision().is_none());
    let all_placements = [
        placements(&first_a),
        placements(&rest),
        placements(&second_a),
        placements(&second_rest),
    ]
    .concat();
    for (wave, captures) in [
        (first_wave, &initial_captures),
        (second_wave, &new_captures),
    ] {
        let actual = all_placements
            .iter()
            .filter(|placement| placement.wave == wave)
            .map(|placement| placement.instance)
            .collect::<Vec<_>>();
        assert_eq!(actual.len(), captures.len());
        assert_eq!(
            actual.iter().copied().collect::<BTreeSet<_>>(),
            captures.keys().copied().collect()
        );
    }
    assert_eq!(
        second_rest
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<Vec<_>>(),
        all_placements
            .iter()
            .map(|placement| placement.entry)
            .collect::<Vec<_>>()
    );
    assert_captured_payloads(second_rest.state(), &all_placements, &all_captures);
    let mut state = second_rest.into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    for (player, total) in [(A, 30), (C, 38), (D, 32)] {
        assert_eq!(state.execution().life_total(player), Some(&total.into()));
    }
}
#[test]
fn actual_departure_discovery_and_later_wave_identity_exhaustion_reject_atomically() {
    use crate::effects::private_qualification::QualificationFault;
    use crate::occurrence::TriggerError;
    let state = prepare_with_exile(true);
    let mut bad = state.clone();
    bad.set_test_check_effect_fault(QualificationFault::Instance(u64::MAX));
    let saved = bad.clone();
    let issued = input(&bad, B, GameCommand::Concede);
    let result = bad.advance(issued.clone());
    assert_eq!(
        result,
        Err(GameError::Effect(EffectError::Trigger(
            TriggerError::InstanceIdentityExhausted
        )))
    );
    assert_eq!(result, bad.advance(issued));
    assert_eq!(bad, saved);
    let departed = apply(&state, B, GameCommand::Concede);
    let placed = ordered(departed.state());
    let mut bad = placed.into_state();
    bad.set_test_check_effect_fault(QualificationFault::Wave(u64::MAX));
    let saved = bad.clone();
    let pending = bad.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = pending.frame() else {
        panic!()
    };
    let issued = input(
        &bad,
        C,
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OrderTriggers(frame.instances().to_vec()),
        },
    );
    let result = bad.advance(issued.clone());
    assert_eq!(
        result,
        Err(GameError::Effect(EffectError::Trigger(
            TriggerError::WaveIdentityExhausted
        )))
    );
    assert_eq!(result, bad.advance(issued));
    assert_eq!(bad, saved);
}

#[test]
fn departure_of_already_placed_controller_prunes_real_stack_without_replaying_historical_group() {
    let state = prepare();
    let placed = ordered(&state);
    assert_eq!(placed.state().execution().window().entries().len(), 2);
    let position = placed.state().turn().unwrap().position();
    let departed = apply(placed.state(), A, GameCommand::Concede);
    assert_eq!(departed.state().turn().unwrap().position(), position);
    assert!(departed.state().execution().window().entries().is_empty());
    assert_eq!(departed.state().pending_decision().unwrap().chooser(), C);
    assert!(placements(&departed).is_empty());
    let placed = ordered(departed.state());
    assert_eq!(
        placements(&placed)
            .iter()
            .map(|p| p.controller)
            .collect::<Vec<_>>(),
        vec![C, C, D]
    );
    assert_eq!(placed.state().execution().window().entries().len(), 3);
}
#[test]
fn missing_or_stale_checks_order_authority_and_operational_exhaustion_are_atomic() {
    use crate::effects::private_qualification::QualificationFault;
    use crate::occurrence::TriggerError;
    let state = prepare();
    assert_eq!(
        state
            .execution()
            .depart_players(&BTreeSet::from([B]), state.turn.as_ref(), None),
        Err(EffectError::UnsupportedDepartureState)
    );
    let placed = ordered(&state);
    for field in 0..3 {
        let mut bad = placed.state().clone();
        match field {
            0 => bad.pending = None,
            1 => {
                bad.pending.as_mut().unwrap().frame =
                    state.pending_decision().unwrap().frame().clone()
            }
            2 => bad.pending.as_mut().unwrap().basis.revision += 1,
            _ => unreachable!(),
        }
        let saved = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        assert_eq!(
            bad.advance(issued.clone()),
            Err(GameError::UnsupportedConcessionScope)
        );
        assert_eq!(
            bad.advance(issued),
            Err(GameError::UnsupportedConcessionScope)
        );
        assert_eq!(bad, saved);
    }
    for fault in [
        QualificationFault::Revision(u64::MAX),
        QualificationFault::Event(u64::MAX),
        QualificationFault::StackEntry(u64::MAX),
    ] {
        let mut bad = placed.state().clone();
        bad.set_test_check_effect_fault(fault);
        let expected = match fault {
            QualificationFault::Revision(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::RevisionExhausted,
            )),
            QualificationFault::Event(_) => {
                GameError::Effect(EffectError::Trigger(TriggerError::EventIdentityExhausted))
            }
            QualificationFault::StackEntry(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::EntryIdentityExhausted,
            )),
            _ => unreachable!(),
        };
        let saved = bad.clone();
        let issued = input(&bad, C, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert_eq!(result, Err(expected), "{fault:?}");
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
    for root in [false, true] {
        let mut bad = state.clone();
        if root {
            bad.set_test_input_revision(u64::MAX)
        } else {
            bad.set_test_next_decision(u64::MAX)
        }
        let saved = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert_eq!(
            result,
            Err(if root {
                GameError::RevisionExhausted
            } else {
                GameError::DecisionIdentityExhausted
            })
        );
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
}

#[test]
fn corrupted_future_capture_cohort_and_unissued_stage_reject_before_departure() {
    use crate::effects::private_qualification::QualificationCheckOrderFault as Fault;
    let state = prepare();
    for fault in [
        Fault::DuplicateFutureMember,
        Fault::ForeignFutureMember,
        Fault::DuplicateFutureController,
        Fault::MissingFutureCapture,
        Fault::UnallocatedWave,
        Fault::UnsupportedStage,
    ] {
        let mut bad = state.clone();
        bad.execution.qualification_corrupt_check_order(C, fault);
        let saved = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let expected = match fault {
            Fault::UnallocatedWave | Fault::UnsupportedStage => {
                GameError::UnsupportedConcessionScope
            }
            _ => GameError::Effect(EffectError::InconsistentProgram),
        };
        let result = bad.advance(issued.clone());
        assert_eq!(result, Err(expected), "{fault:?}");
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
}

#[test]
fn real_departure_trigger_order_preserves_nonzero_pass_prefix_and_unrelated_lower_stack() {
    let mut cards = vec![
        card(
            99,
            C,
            ObjectDestination::Battlefield { controller: B },
            creature(),
        ),
        card(
            70,
            C,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::AdjustControllerLife {
                        adjustment: LifeAdjustment::Gain(7.into()),
                    }],
                ),
            ),
        ),
    ];
    for (id, owner) in [(2, A), (3, A), (4, C), (5, C), (6, D)] {
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller: owner },
            watcher(
                id,
                OrdinaryCondition::LeavesBattlefield(ObjectRelation::AnyCreature),
            ),
        ));
    }
    let mut state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    for actor in [A, B] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        C,
        GameCommand::CastUntargeted {
            spell: object(&state, 70),
            payment: None,
        },
    )
    .into_state();
    let lower = state.execution().window().entries()[0].clone();
    for actor in [C, D, A] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert_eq!(holder(&state), B);
    assert_eq!(
        state
            .execution()
            .window()
            .qualification_priority_pass_count(),
        Some(3)
    );
    let discovered = apply(&state, B, GameCommand::Concede);
    let actual = discovered
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some((waiting.id, waiting.as_ref().clone())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(actual.len(), 5);
    assert_eq!(
        discovered
            .state()
            .execution()
            .window()
            .qualification_priority_pass_count(),
        Some(2)
    );
    assert_eq!(
        discovered
            .state()
            .execution()
            .waiting_triggers()
            .map(|waiting| (waiting.id, waiting.clone()))
            .collect::<BTreeMap<_, _>>(),
        actual
    );
    assert_eq!(discovered.state().pending_decision().unwrap().chooser(), A);
    assert!(matches!(
        discovered.state().execution().window().phase(),
        WindowPhase::Priority(PriorityPhase::RequiresChecks { candidate: C })
    ));
    let departed = apply(discovered.state(), D, GameCommand::Concede);
    assert_eq!(
        departed.state().execution().window().entries(),
        std::slice::from_ref(&lower)
    );
    assert_eq!(
        departed
            .state()
            .execution()
            .waiting_triggers()
            .map(|waiting| (waiting.id, waiting.clone()))
            .collect::<BTreeMap<_, _>>(),
        actual
            .into_iter()
            .filter(|(_, waiting)| waiting.controller != D)
            .collect()
    );
    assert_eq!(
        departed
            .state()
            .execution()
            .window()
            .qualification_priority_pass_count(),
        Some(1)
    );
    let a = ordered(departed.state());
    assert_eq!(
        a.state()
            .execution()
            .window()
            .qualification_priority_pass_count(),
        Some(0)
    );
    let c = ordered(a.state());
    let expected = std::iter::once(lower.id())
        .chain(
            placements(&a)
                .iter()
                .chain(placements(&c).iter())
                .map(|placement| placement.entry),
        )
        .collect::<Vec<_>>();
    assert_eq!(
        c.state()
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(holder(c.state()), C);
    // Real trigger placement changed the stack, so both surviving players pass afresh.
    assert_eq!(
        c.state()
            .execution()
            .window()
            .qualification_priority_pass_count(),
        Some(0)
    );
    let first = apply(c.state(), C, GameCommand::Pass);
    assert_eq!(first.state().execution().life_total(C), Some(&20.into()));
    assert_eq!(first.state().execution().window().entries().len(), 5);
    let second = apply(first.state(), A, GameCommand::Pass);
    assert_eq!(second.state().execution().life_total(C), Some(&24.into()));
    assert_eq!(second.state().execution().window().entries().len(), 4);
    let mut state = second.into_state();
    while !state.execution().window().entries().is_empty() {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert_eq!(state.execution().life_total(A), Some(&25.into()));
    assert_eq!(state.execution().life_total(C), Some(&36.into()));
    assert_eq!(state.execution().life_total(D), Some(&20.into()));
}
