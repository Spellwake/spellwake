//! Actual CR800.4g trigger continuation, without an invented choice or cursor.
use super::*;
use crate::effects::{OptionalEffectChoice, ResolutionChoiceState};
const D: SeatId = SeatId(77);

fn offered() -> GameState {
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::LeavesBattlefield(
            crate::program::ObjectRelation::AnyCreature,
        ),
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
    let mut state = root(
        vec![
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
                creature(),
            ),
        ],
        &[20, 20, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    state = apply(&state, B, GameCommand::Concede).into_state();
    for actor in [A, C, D] {
        state = apply(&state, actor, GameCommand::Pass).into_state();
    }
    state
}
fn replacement(state: &GameState, player: SeatId) -> GameCommand {
    GameCommand::Reply {
        decision: state.pending_decision().unwrap().id(),
        choice: DecisionReply::ReplacementChooser(player),
    }
}

#[test]
fn controller_selects_nonfirst_opponent_then_that_survivor_makes_the_original_optional_choice() {
    let state = offered();
    let pending = state.pending_decision().unwrap();
    let RuleFrame::ReplacementChooser(frame) = pending.frame() else {
        panic!("replacement chooser")
    };
    assert_eq!(pending.chooser(), A);
    assert_eq!(frame.departed(), B);
    assert_eq!(frame.candidates(), &[C, D]);
    assert_eq!(frame.instruction(), 0);
    let entry = frame.entry();
    let Some(ResolutionChoiceState::ChooserSubstitution {
        cursor,
        departed,
        controller,
        candidates,
        options,
    }) = state.execution().resolution_choice_state()
    else {
        panic!("substitution owner")
    };
    assert_eq!((cursor.entry(), cursor.instruction()), (entry, 0));
    assert_eq!((departed, controller), (B, A));
    assert_eq!(candidates, &[C, D]);
    let original_options = options.clone();
    let inspected = crate::test_support::projection::pending_work(state.execution()).unwrap();
    assert!(inspected.to_string().contains("ChooserSubstitution"));
    assert_eq!(
        inspected["work"]["options"],
        serde_json::json!(["Take", "Decline"])
    );
    let counters = state.execution().operational_counters();
    let candidate = apply(&state, A, replacement(&state, D));
    assert_eq!(
        candidate.state().execution().operational_counters(),
        counters,
        "selecting a chooser allocates no Magic event, trigger or effect identity"
    );
    assert_eq!(
        candidate.rules_journal().records.as_slice(),
        &[RulesRecord::ChooserReplaced {
            entry,
            instruction: 0,
            departed: B,
            controller: A,
            candidates: vec![C, D],
            selected: D,
        }]
    );
    let next = candidate.into_state();
    assert_eq!(next.basis().revision, state.basis().revision + 1);
    assert_ne!(next.pending_decision().unwrap().id(), pending.id());
    assert_eq!(next.pending_decision().unwrap().chooser(), D);
    let RuleFrame::OptionalEffect(frame) = next.pending_decision().unwrap().frame() else {
        panic!("original choice")
    };
    assert_eq!((frame.entry(), frame.instruction()), (entry, 0));
    assert_eq!(frame.options(), &original_options);
    assert!(matches!(
        next.execution().resolution_choice_state(),
        Some(ResolutionChoiceState::Optional { chooser: D, .. })
    ));
    for (choice, life) in [
        (OptionalEffectChoice::Take, 27),
        (OptionalEffectChoice::Decline, 20),
    ] {
        let result = apply(
            &next,
            D,
            GameCommand::Reply {
                decision: next.pending_decision().unwrap().id(),
                choice: DecisionReply::OptionalEffect(choice),
            },
        );
        assert_eq!(result.state().execution().life_total(A), Some(&life.into()));
        assert!(result.state().pending_decision().is_none());
        assert!(result.state().execution().window().entries().is_empty());
        assert_eq!(
            result
                .rules_journal()
                .records
                .iter()
                .filter(|record| matches!(
                    record,
                    RulesRecord::OptionalEffectOutcome { chooser: D, .. }
                ))
                .count(),
            1
        );
    }
}

#[test]
fn replacement_rejects_wrong_controller_departed_foreign_and_nonopponent_selections_atomically() {
    let state = offered();
    let before = state.clone();
    assert_eq!(
        state.advance(input(&state, C, replacement(&state, D))),
        Err(GameError::WrongChooser)
    );
    for player in [A, B, SeatId(999)] {
        assert_eq!(
            state.advance(input(&state, A, replacement(&state, player))),
            Err(GameError::Effect(EffectError::SelectionOutsideDomain))
        );
        assert_eq!(state, before);
    }
    let next = apply(&state, A, replacement(&state, D)).into_state();
    assert_eq!(
        next.advance(input(&state, A, replacement(&state, D))),
        Err(GameError::StaleRevision)
    );
    assert_eq!(
        next.advance(input(&next, A, replacement(&state, D))),
        Err(GameError::WrongDecision)
    );
}

#[test]
fn substitution_departure_preserves_owner_and_rolls_back_late_allocator_failure() {
    use crate::effects::private_qualification::QualificationFault;
    let state = offered();
    let next = apply(&state, A, replacement(&state, D)).into_state();
    for pending in [&state, &next] {
        let departed = apply(pending, C, GameCommand::Concede).into_state();
        assert_eq!(departed.execution().window().participants(), &[A, D]);
        assert_ne!(
            departed.pending_decision().unwrap().id(),
            pending.pending_decision().unwrap().id()
        );
        assert_eq!(
            departed.pending_decision().unwrap().chooser(),
            pending.pending_decision().unwrap().chooser()
        );
    }
    for fault in 0..3 {
        let mut state = state.clone();
        match fault {
            0 => state.set_test_input_revision(u64::MAX),
            1 => state.set_test_check_effect_fault(QualificationFault::Revision(u64::MAX)),
            2 => state.set_test_next_decision(u64::MAX),
            _ => unreachable!(),
        }
        let before = state.clone();
        let result = state.advance(input(&state, A, replacement(&state, D)));
        assert!(result.is_err());
        assert_eq!(
            result,
            state.advance(input(&state, A, replacement(&state, D)))
        );
        assert_eq!(state, before);
    }
}

#[test]
fn executor_rejects_every_mutated_replacement_token_without_adopting_a_prefix() {
    use crate::{effects::EffectDecision, object_identity::GameObjects, stack_window::StackWindow};
    let state = offered();
    let RuleFrame::ReplacementChooser(frame) = state.pending_decision().unwrap().frame() else {
        panic!("replacement frame")
    };
    // A separate native allocator supplies a foreign entry token for this control.
    let mut cards = GameObjects::new(0);
    for id in [1001, 1002] {
        cards = cards
            .seed_card(cards.revision(), CardId(id), A, ObjectDestination::Hand)
            .unwrap()
            .next;
    }
    let objects = cards.iter().map(|object| object.id()).collect::<Vec<_>>();
    let mut window = StackWindow::new(vec![A, C], A, cards).unwrap();
    for object in objects {
        window = window.finish_checks(window.revision()).unwrap();
        window = window
            .record_card_spell(window.revision(), A, object)
            .unwrap();
    }
    let other = window.entries()[1].id();
    assert_ne!(other, frame.entry());
    let execution = state.execution();
    let before = execution.clone();
    for (field, frame) in frame.qualification_mutations(other) {
        let result = execution.resume_rule(
            execution.revision(),
            &EffectDecision::ReplacementChooser(frame),
            A,
            DecisionReply::ReplacementChooser(D),
            state.turn(),
        );
        assert_eq!(
            result,
            Err(EffectError::WrongReplacementChooserFrame),
            "mutated {field}"
        );
        assert_eq!(*execution, before);
    }
}
