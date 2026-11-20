//! Departure before the active player selects their cleanup discards.
use super::*;
use crate::checks::{CheckChoice, CleanupDiscardChoice};
use crate::occurrence::EventCause;

const D: SeatId = SeatId(77);

fn hand() -> Vec<SeedCard> {
    (1..=9)
        .map(|id| {
            card(
                id,
                A,
                ObjectDestination::Hand,
                Program::artifact(None, vec![]),
            )
        })
        .collect()
}

fn cleanup(cards: Vec<SeedCard>) -> GameState {
    root(cards, &[20; 4], Some(Step::Cleanup), vec![A, B, C, D])
}

fn frame(state: &GameState) -> &CleanupDiscardChoice {
    match state.pending_decision().unwrap().frame() {
        RuleFrame::Checks(
            CheckDecision::CleanupDiscard(frame) | CheckDecision::CleanupSelect(frame),
        ) => frame,
        _ => panic!("expected a cleanup discard choice"),
    }
}

fn discard(state: &GameState) -> GameCommand {
    GameCommand::Reply {
        decision: state.pending_decision().unwrap().id(),
        choice: DecisionReply::Discard(frame(state).domain()[..frame(state).required()].to_vec()),
    }
}

#[test]
fn bystander_departures_keep_the_active_hand_and_discard_event_then_accept_one_real_reply() {
    let state = cleanup(hand());
    let original_frame = frame(&state);
    let stale = discard(&state);

    let departed = apply(&state, B, GameCommand::Concede);
    assert!(departed.check_journal().choices.is_empty());
    assert!(departed.check_journal().moves.is_empty());
    let departed = apply(departed.state(), D, GameCommand::Concede);
    let refreshed = frame(departed.state());
    assert_eq!(refreshed.event(), original_frame.event());
    assert_eq!(refreshed.boundary(), original_frame.boundary());
    assert_eq!(refreshed.domain(), original_frame.domain());
    assert_eq!(refreshed.required(), 2);
    assert_eq!(refreshed.chooser(), A);
    assert_ne!(refreshed.revision(), original_frame.revision());
    assert_eq!(
        departed.state().advance(input(departed.state(), A, stale)),
        Err(GameError::WrongDecision)
    );

    let finished = apply(departed.state(), A, discard(departed.state()));
    assert_eq!(finished.check_journal().choices.len(), 1);
    assert!(matches!(
        &finished.check_journal().choices[0].choice,
        CheckChoice::CleanupDiscard { ordered } if ordered == &original_frame.domain()[..2]
    ));
    assert_eq!(finished.check_journal().moves.len(), 2);
    assert_eq!(finished.state().turn().unwrap().position().number, 2);
    assert_eq!(
        finished.state().execution().window().active_player(),
        Some(C)
    );
    assert_eq!(holder(finished.state()), C);
}

#[test]
fn departing_active_player_finishes_cleanup_checks_and_repeats_after_a_surviving_trigger() {
    use crate::characteristics::PowerToughness;
    use crate::checks::CheckMoveCause;
    use crate::program::ObjectRelation;

    let mut cards = hand();
    let mut witness = Program::artifact(None, vec![]);
    witness.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::LeavesBattlefield(ObjectRelation::AnyCreature),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(7.into()),
            }],
        ),
    });
    cards.extend([
        card(
            19,
            D,
            ObjectDestination::Battlefield { controller: D },
            witness,
        ),
        card(
            20,
            C,
            ObjectDestination::Battlefield { controller: C },
            Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        ),
        card(
            21,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SingleCreature,
                    [
                        (1, ResolvedDuration::UntilEndOfTurn),
                        (-1, ResolvedDuration::NoStatedDuration),
                    ]
                    .into_iter()
                    .map(
                        |(toughness, duration)| Instruction::CreateContinuousEffect {
                            parts: ResolvedPartsSpec(vec![ResolvedPartSpec::ModifyPT(
                                PowerToughness {
                                    power: 0.into(),
                                    toughness: toughness.into(),
                                },
                            )]),
                            duration,
                        },
                    )
                    .collect(),
                ),
            ),
        ),
    ]);
    let state = root(cards, &[20; 4], Some(Step::End), vec![A, B, C, D]);
    let victim = object(&state, 20);
    let mut state = cast_and_resolve(state, A, 21, victim);
    for _ in 0..4 {
        if state.pending_decision().is_some() {
            break;
        }
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert_eq!(frame(&state).chooser(), A);

    let left = apply(&state, A, GameCommand::Concede);
    assert!(left.check_journal().choices.is_empty());
    let [movement] = left.check_journal().moves.as_slice() else {
        panic!("expected exactly one state-based move")
    };
    assert_eq!(movement.event.pass(), 1);
    assert_eq!(movement.cause, CheckMoveCause::StateBasedAction);
    assert_eq!(movement.before.id(), victim);
    assert_eq!(movement.after.zone(), ObjectZone::Graveyard(C));
    let expiration_events = left
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| {
            let RulesRecord::Event(event) = record else {
                return None;
            };
            matches!(event.cause, EventCause::CleanupCharacteristics { .. }).then_some(event)
        })
        .collect::<Vec<_>>();
    assert_eq!(expiration_events.len(), 1);
    assert!(expiration_events[0].occurrences.iter().any(|o| matches!(
        &o.value, RulesOccurrence::CleanupCharacteristics { expired_effects, .. }
        if expired_effects.len() == 1
    )));
    let mut state = left.into_state();
    assert_eq!(state.turn().unwrap().position().step, Step::Cleanup);
    assert_eq!(state.execution().window().active_player(), None);
    assert_eq!(holder(&state), B);
    assert_eq!(state.execution().window().entries().len(), 1);
    let mut repeated = false;
    for _ in 0..9 {
        if state.turn().unwrap().position().number == 2 {
            break;
        }
        let next = apply(&state, holder(&state), GameCommand::Pass);
        for record in &next.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                assert!(!matches!(
                    event.cause,
                    EventCause::CleanupCharacteristics { .. }
                ));
                repeated |= event.occurrences.iter().any(|occurrence| {
                    matches!(
                        occurrence.value,
                        RulesOccurrence::StepBegan { position, active: None }
                            if position.step == Step::Cleanup && position.cleanup_iteration == 1
                    )
                });
            }
        }
        state = next.into_state();
    }
    assert!(repeated);
    assert_eq!(state.execution().life_total(D), Some(&27.into()));
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(state.turn().unwrap().position().number, 2);
    assert_eq!(state.execution().window().active_player(), Some(B));
    assert_eq!(holder(&state), B);
}

#[test]
fn departure_of_last_replacement_source_changes_precommit_selection_to_ordinary_discard() {
    let mut cards = hand();
    cards.push(card(
        20,
        B,
        ObjectDestination::Battlefield { controller: B },
        crate::game::concession_tests::check_replacements::replacement_source(
            ZoneActionKind::CleanupDiscard,
        ),
    ));
    let state = cleanup(cards);
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::CleanupSelect(_))
    ));
    let left = apply(&state, B, GameCommand::Concede);
    assert!(left.check_journal().choices.is_empty());
    assert!(matches!(
        left.state().pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::CleanupDiscard(_))
    ));
    assert_eq!(frame(left.state()).domain(), frame(&state).domain());
    assert_eq!(frame(left.state()).event(), frame(&state).event());
    let finished = apply(left.state(), A, discard(left.state()));
    assert_eq!(finished.check_journal().moves.len(), 2);
}

#[test]
fn bystander_departure_preserves_the_selected_cleanup_replacement_and_its_chooser() {
    let mut cards = hand();
    cards.push(card(
        20,
        B,
        ObjectDestination::Battlefield { controller: B },
        crate::game::concession_tests::check_replacements::replacement_source(
            ZoneActionKind::CleanupDiscard,
        ),
    ));
    let state = cleanup(cards);
    let state = apply(&state, A, discard(&state)).into_state();
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::ZoneReplacement(_)
    ));
    // The selected player's departure removes the hand before any discard is committed.
    let left_active = apply(&state, A, GameCommand::Concede);
    assert!(
        !left_active
            .state()
            .execution()
            .window()
            .participants()
            .contains(&A)
    );
    assert!(left_active.check_journal().moves.is_empty());

    let original = state.pending_decision().unwrap();
    let held = state.execution().check_zone_replacement().unwrap();
    // The source leaves before anyone accepts its offer. The chosen cards
    // still enter the graveyard in the order their owner supplies.
    let source_left = apply(&state, B, GameCommand::Concede);
    assert!(source_left.check_journal().moves.is_empty());
    let order = source_left.state().pending_decision().unwrap();
    assert_eq!(order.chooser(), A);
    assert_ne!(order.id(), original.id());
    let RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) = order.frame() else {
        panic!("expected an order for the previously selected cards");
    };
    let selected: BTreeSet<_> = held.intents().iter().map(|intent| intent.before).collect();
    assert_eq!(
        frame.domain().iter().copied().collect::<BTreeSet<_>>(),
        selected
    );
    let finished = apply(
        source_left.state(),
        A,
        GameCommand::Reply {
            decision: order.id(),
            choice: DecisionReply::OrderGraveyard(frame.domain().to_vec()),
        },
    );
    assert_eq!(
        finished
            .check_journal()
            .moves
            .iter()
            .map(|m| m.before.id())
            .collect::<BTreeSet<_>>(),
        selected
    );
    assert!(
        finished
            .check_journal()
            .moves
            .iter()
            .all(|m| m.after.zone() == ObjectZone::Graveyard(A))
    );
    let left = apply(&state, D, GameCommand::Concede);
    let next = left.state();
    let fresh = next.pending_decision().unwrap();
    let current = next.execution().check_zone_replacement().unwrap();
    assert_ne!(original.id(), fresh.id());
    assert_eq!(original.chooser(), fresh.chooser());
    assert_eq!(held.intents(), current.intents());
    assert_eq!(held.history(), current.history());
    assert_eq!(held.cause(), current.cause());
    let RuleFrame::ZoneReplacement(frame) = fresh.frame() else {
        panic!("expected replacement");
    };
    fn replacement_reply(frame: &crate::effects::ZoneReplacementFrame) -> DecisionReply {
        DecisionReply::ZoneReplacement(match frame.request() {
            ZoneReplacementRequest::Select { applications } => {
                ZoneReplacementReply::Select(applications[0])
            }
            ZoneReplacementRequest::ConfirmOptional { .. } => {
                ZoneReplacementReply::Optional(ZoneOptionalAnswer::Decline)
            }
        })
    }
    let reply = |id| GameCommand::Reply {
        decision: id,
        choice: replacement_reply(frame),
    };
    assert_eq!(
        next.advance(input(next, original.chooser(), reply(original.id()))),
        Err(GameError::WrongDecision)
    );
    let mut answered = apply(next, fresh.chooser(), reply(fresh.id()));
    assert!(
        answered
            .state()
            .advance(input(answered.state(), fresh.chooser(), reply(fresh.id())))
            .is_err()
    );
    // The replacement allows a separate choice for each object. Decline each,
    // then submit the survivor's actual ordering choice for the simultaneous move.
    for _ in 0..8 {
        let Some(decision) = answered.state().pending_decision() else {
            break;
        };
        assert_eq!(decision.chooser(), A);
        assert!(answered.check_journal().moves.is_empty());
        let choice = match decision.frame() {
            RuleFrame::ZoneReplacement(frame) => replacement_reply(frame),
            RuleFrame::Checks(CheckDecision::GraveyardOrder(frame)) => {
                assert_eq!(
                    frame.domain().iter().copied().collect::<BTreeSet<_>>(),
                    selected
                );
                DecisionReply::OrderGraveyard(frame.domain().to_vec())
            }
            _ => panic!("unexpected cleanup continuation"),
        };
        answered = apply(
            answered.state(),
            A,
            GameCommand::Reply {
                decision: decision.id(),
                choice,
            },
        );
    }
    let moves = &answered.check_journal().moves;
    assert_eq!(moves.len(), 2);
    assert_eq!(
        moves.iter().map(|m| m.before.id()).collect::<BTreeSet<_>>(),
        selected
    );
    assert!(
        moves
            .iter()
            .all(|m| m.after.zone() == ObjectZone::Graveyard(A))
    );
    assert_eq!(holder(answered.state()), B);
    assert!(
        !answered
            .state()
            .execution()
            .window()
            .participants()
            .contains(&D)
    );
}
