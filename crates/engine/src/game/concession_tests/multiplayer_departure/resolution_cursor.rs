//! Exact plain cursor choices retain locals and target semantics after departure.
use super::*;
use crate::activated::{ActivatedAbility, ActivatedAbilityId, SourceCost};
use crate::effects::CopyTargetChoice;
use crate::program::SpellTargetFilter;
use crate::stack_window::StackKind;
const D: SeatId = SeatId(77);
fn answer(state: &GameState, choice: DecisionReply) -> NativeCandidate {
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
fn type_pending(ability: bool, lower: bool) -> (GameState, RuleFrame) {
    let body = ExecutableBody::new(
        if ability {
            BodyTarget::SinglePlayer
        } else {
            BodyTarget::None
        },
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
            if ability {
                Instruction::AdjustTargetLife {
                    adjustment: LifeAdjustment::Gain(3.into()),
                }
            } else {
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(3.into()),
                }
            },
        ],
    );
    let source = if ability {
        card(
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
        )
    } else {
        card(
            1,
            A,
            ObjectDestination::Hand,
            Program::instant(ManaCost::free(), body),
        )
    };
    let mut cards = vec![source];
    if lower {
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
    if lower {
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
    let command = if ability {
        GameCommand::ActivateAbility {
            source: object(&state, 1),
            ability: ActivatedAbilityId(1),
            target: TargetRef::Player(A),
            payment: None,
        }
    } else {
        GameCommand::CastUntargeted {
            spell: object(&state, 1),
            payment: None,
        }
    };
    state = apply(&state, A, command).into_state();
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    let old = state.pending_decision().unwrap().frame().clone();
    let human = state
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    state = answer(&state, DecisionReply::CreatureType(human)).into_state();
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    (state, old)
}
fn copy_pending() -> GameState {
    let mut state = root(
        vec![
            card(
                1,
                A,
                ObjectDestination::Hand,
                Program::instant(
                    ManaCost::free(),
                    ExecutableBody::new(
                        BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
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
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    state = apply(&state, A, GameCommand::Pass).into_state();
    state = apply(
        &state,
        B,
        GameCommand::Cast {
            spell: object(&state, 2),
            target: TargetRef::Player(C),
        },
    )
    .into_state();
    let original = state.execution().window().entries()[0].id();
    for actor in [B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state = apply(
        &state,
        A,
        GameCommand::Cast {
            spell: object(&state, 1),
            target: TargetRef::Stack(original),
        },
    )
    .into_state();
    for actor in [A, B, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::CopyTargets(_)
    ));
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    state
}
fn finish_stack(mut state: GameState) -> GameState {
    for _ in 0..30 {
        if state.execution().window().entries().is_empty() {
            return state;
        }
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    panic!("bounded stack finish")
}
fn no_copy(candidate: &NativeCandidate) {
    assert!(!candidate.rules_journal().records.iter().any(|record| matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(value.value,RulesOccurrence::CopyCreated {..})))));
}
#[test]
fn type_choice_retains_nonempty_locals_and_prefix_even_when_ability_source_owner_leaves() {
    for ability in [false, true] {
        let (state, _) = type_pending(ability, false);
        let saved = state.execution().resolution_state().unwrap();
        assert_eq!(saved.instruction(), 2);
        assert_eq!(saved.locals().len(), 1);
        let old = state.pending_decision().unwrap();
        let next = apply(&state, B, GameCommand::Concede).into_state();
        assert_eq!(next.execution().resolution_state(), Some(saved));
        assert_ne!(next.pending_decision().unwrap().id(), old.id());
        if ability {
            assert!(
                next.execution()
                    .window()
                    .cards()
                    .current_identity(CardId(1))
                    .is_none()
            );
        }
        let elf = next
            .execution()
            .creature_vocabulary()
            .resolve_label("Elf")
            .unwrap();
        let done = answer(&next, DecisionReply::CreatureType(elf));
        assert_eq!(done.state().execution().life_total(A), Some(&25.into()));
        assert!(done.state().pending_decision().is_none());
        assert_eq!(done.rules_journal().records.iter().filter(|record| matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|value| matches!(value.value,RulesOccurrence::CreatureTypeChosen {slot,..} if slot==CreatureTypeSlot::new(1))))).count(),1);
    }
}
#[test]
fn type_controller_departure_cancels_only_remaining_cursor_and_lower_spell_resolves() {
    let (state, _) = type_pending(true, true);
    let candidate = apply(&state, A, GameCommand::Concede);
    assert!(candidate.state().pending_decision().is_none());
    assert!(candidate.state().execution().resolution_state().is_none());
    assert_eq!(candidate.state().execution().window().entries().len(), 1);
    let done = finish_stack(candidate.into_state());
    assert_eq!(done.execution().life_total(D), Some(&25.into()));
    assert_eq!(done.execution().life_total(A), Some(&22.into()));
}
#[test]
fn copy_retain_preserves_now_illegal_old_target_and_change_selects_live_target() {
    let state = copy_pending();
    let old = state.pending_decision().unwrap();
    for departing in [C, D] {
        let next = apply(&state, departing, GameCommand::Concede).into_state();
        assert_ne!(next.pending_decision().unwrap().id(), old.id());
        assert_eq!(
            next.execution().resolution_state(),
            state.execution().resolution_state()
        );
        assert!(matches!(
            next.pending_decision().unwrap().frame(),
            RuleFrame::CopyTargets(_)
        ));
        if departing == C {
            let saved = next.clone();
            assert!(
                next.advance(input(
                    &next,
                    A,
                    GameCommand::Reply {
                        decision: next.pending_decision().unwrap().id(),
                        choice: DecisionReply::CopyTargets(CopyTargetChoice::Change(
                            TargetRef::Player(C)
                        ))
                    }
                ))
                .is_err()
            );
            assert_eq!(next, saved);
        }
        for (choice, target, life) in [
            (CopyTargetChoice::Retain, C, 25),
            (CopyTargetChoice::Change(TargetRef::Player(A)), A, 21),
        ] {
            let candidate = answer(&next, DecisionReply::CopyTargets(choice));
            let copy = candidate
                .state()
                .execution()
                .window()
                .entries()
                .last()
                .unwrap();
            assert!(matches!(copy.kind(), StackKind::SpellCopy { .. }));
            assert_eq!(
                copy.targets().targets()[0].choice().target,
                TargetRef::Player(target)
            );
            let done = finish_stack(candidate.into_state());
            assert_eq!(done.execution().life_total(A), Some(&life.into()));
            let c_total = if departing == C {
                20
            } else if target == C {
                12
            } else {
                16
            };
            assert_eq!(done.execution().life_total(C), Some(&c_total.into()));
        }
    }
}

#[test]
fn original_spell_departure_skips_unavailable_copy_then_runs_tail_without_prefix_replay() {
    let state = copy_pending();
    let candidate = apply(&state, B, GameCommand::Concede);
    no_copy(&candidate);
    assert!(candidate.state().pending_decision().is_none());
    assert!(candidate.state().execution().window().entries().is_empty());
    assert_eq!(
        candidate.state().execution().life_total(A),
        Some(&25.into())
    );
}
#[test]
fn copy_controller_departure_leaves_lower_original_to_resolve_normally() {
    let state = copy_pending();
    let candidate = apply(&state, A, GameCommand::Concede);
    no_copy(&candidate);
    assert!(candidate.state().pending_decision().is_none());
    assert_eq!(candidate.state().execution().window().entries().len(), 1);
    let done = finish_stack(candidate.into_state());
    assert_eq!(done.execution().life_total(C), Some(&16.into()));
    assert_eq!(done.execution().life_total(A), Some(&22.into()));
}
#[test]
fn issued_frame_correspondence_stale_tokens_and_late_allocator_failures_are_atomic() {
    use crate::effects::private_qualification::QualificationFault;
    let (state, old) = type_pending(false, false);
    assert_eq!(
        state
            .execution()
            .depart_players(&BTreeSet::from([B]), state.turn.as_ref(), None),
        Err(EffectError::UnsupportedDepartureState)
    );
    for fault in 0..3 {
        let mut bad = state.clone();
        match fault {
            0 => bad.pending = None,
            1 => bad.pending.as_mut().unwrap().frame = old.clone(),
            2 => bad.pending.as_mut().unwrap().basis.revision += 1,
            _ => unreachable!(),
        }
        let before = bad.clone();
        assert!(bad.advance(input(&bad, B, GameCommand::Concede)).is_err());
        assert_eq!(bad, before);
    }
    for current in [state, copy_pending()] {
        for fault in 0..3 {
            let mut bad = current.clone();
            match fault {
                0 => bad.set_test_input_revision(u64::MAX),
                1 => bad.set_test_check_effect_fault(QualificationFault::Revision(u64::MAX)),
                2 => bad.set_test_next_decision(u64::MAX),
                _ => unreachable!(),
            }
            let before = bad.clone();
            let issued = input(&bad, D, GameCommand::Concede);
            let result = bad.advance(issued.clone());
            assert!(result.is_err());
            assert_eq!(result, bad.advance(issued));
            assert_eq!(bad, before);
        }
    }
    let (state, _) = type_pending(false, false);
    let next = apply(&state, D, GameCommand::Concede).into_state();
    let human = next
        .execution()
        .creature_vocabulary()
        .resolve_label("Human")
        .unwrap();
    assert_eq!(
        next.advance(input(
            &next,
            A,
            GameCommand::Reply {
                decision: state.pending_decision().unwrap().id(),
                choice: DecisionReply::CreatureType(human)
            }
        )),
        Err(GameError::WrongDecision)
    );
}
