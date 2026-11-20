//! Actual precommit Legend owner departure, with exact accepted cohorts only.
use super::*;
use crate::checks::{CheckDecision, LegendChoice};
use spellwake_contracts::names::{EnglishCardName, ObjectNames};
const D: SeatId = SeatId(77);
fn legend_card(id: u64, owner: SeatId, controller: SeatId, name: &str) -> SeedCard {
    let mut program = Program::artifact(None, vec![]);
    program.supertypes.insert(Supertype::Legendary);
    program.names = ObjectNames::new(vec![EnglishCardName::new(name.into()).unwrap()]).unwrap();
    card(
        id,
        owner,
        ObjectDestination::Battlefield { controller },
        program,
    )
}
fn prepare(second_owner: SeatId) -> GameState {
    root(
        vec![
            legend_card(1, A, A, "Alpha"),
            legend_card(2, second_owner, A, "Alpha"),
            legend_card(3, C, C, "Gamma"),
            legend_card(4, C, C, "Gamma"),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    )
}
fn frame(state: &GameState) -> &LegendChoice {
    let RuleFrame::Checks(CheckDecision::Legend(frame)) = state.pending_decision().unwrap().frame()
    else {
        panic!("Legend")
    };
    frame
}
fn keep(state: &GameState, id: u64) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    let frame = frame(state);
    let conflict = frame
        .conflicts()
        .iter()
        .find(|c| c.domain().contains(&object(state, id)))
        .unwrap();
    apply(
        state,
        frame.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::KeepLegend {
                conflict: conflict.id(),
                keep: object(state, id),
            },
        },
    )
}
fn zone(state: &GameState, id: u64) -> ObjectZone {
    state
        .execution()
        .window()
        .cards()
        .get(object(state, id))
        .unwrap()
        .zone()
}
#[test]
fn unrelated_departures_preserve_original_accepted_keep_then_commit_each_simultaneous_move_once() {
    let state = prepare(A);
    let accepted = keep(&state, 1);
    let choice = accepted.check_journal().choices[0].clone();
    let state = accepted.into_state();
    assert_eq!(frame(&state).chooser(), C);
    let state = apply(&state, B, GameCommand::Concede).into_state();
    let state = apply(&state, D, GameCommand::Concede).into_state();
    assert_eq!(frame(&state).chooser(), C);
    let finished = keep(&state, 3);
    assert_eq!(finished.check_journal().choices.len(), 1);
    assert_eq!(finished.check_journal().moves.len(), 2);
    assert_eq!(
        finished
            .check_journal()
            .moves
            .iter()
            .map(|m| m.before.card_id().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([CardId(2), CardId(4)])
    );
    assert!(
        matches!(choice.choice,crate::checks::CheckChoice::LegendKeep{keep,..} if keep==object(&state,1))
    );
    assert_eq!(zone(finished.state(), 1), ObjectZone::Battlefield);
    assert_eq!(zone(finished.state(), 2), ObjectZone::Graveyard(A));
    assert_eq!(zone(finished.state(), 3), ObjectZone::Battlefield);
    assert_eq!(zone(finished.state(), 4), ObjectZone::Graveyard(C));
    assert!(finished.state().pending_decision().is_none());
    assert_eq!(holder(finished.state()), A);
}
#[test]
fn current_legend_controller_departure_skips_actual_gone_cohort_without_fake_choice() {
    let state = prepare(A);
    let old = state.pending_decision().unwrap().id();
    let departed = apply(&state, A, GameCommand::Concede);
    assert!(departed.check_journal().choices.is_empty());
    let state = departed.into_state();
    assert_eq!(frame(&state).chooser(), C);
    assert_ne!(state.pending_decision().unwrap().id(), old);
    let finished = keep(&state, 3);
    assert_eq!(finished.check_journal().moves.len(), 1);
    assert_eq!(holder(finished.state()), B);
    assert_eq!(zone(finished.state(), 4), ObjectZone::Graveyard(C));
    let mut state = finished.into_state();
    let mut ownerless_steps = 0;
    for _ in 0..40 {
        if state.turn().unwrap().position().number == 2 {
            break;
        }
        let next = apply(&state, holder(&state), GameCommand::Pass);
        for record in &next.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::StepBegan { position, active } = occurrence.value
                        && position.number == 1
                    {
                        assert_eq!(active, None);
                        ownerless_steps += 1;
                    }
                }
            }
        }
        state = next.into_state();
    }
    assert!(ownerless_steps > 0);
    assert_eq!(state.turn().unwrap().position().number, 2);
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(state.execution().window().active_player(), Some(B));
    assert_eq!(holder(&state), B);
}
#[test]
fn unanswered_source_owner_loss_collapses_current_cohort_and_issues_next_actual_chooser() {
    let state = prepare(B);
    let departed = apply(&state, B, GameCommand::Concede);
    assert!(departed.check_journal().choices.is_empty());
    assert!(departed.check_journal().moves.is_empty());
    assert_eq!(frame(departed.state()).chooser(), C);
    assert_eq!(zone(departed.state(), 1), ObjectZone::Battlefield);
    let finished = keep(departed.state(), 3);
    assert_eq!(finished.check_journal().moves.len(), 1);
    assert_eq!(zone(finished.state(), 1), ObjectZone::Battlefield);
}
#[test]
fn future_controller_loss_keeps_current_unanswered_cohort_and_commits_no_departed_legend_action() {
    let state = prepare(A);
    let departed = apply(&state, C, GameCommand::Concede);
    assert_eq!(frame(departed.state()).chooser(), A);
    assert!(departed.check_journal().choices.is_empty());
    let finished = keep(departed.state(), 1);
    assert_eq!(finished.check_journal().moves.len(), 1);
    assert_eq!(zone(finished.state(), 2), ObjectZone::Graveyard(A));
}
#[test]
fn departure_of_the_other_legend_keeps_the_accepted_card_and_continues_the_next_choice() {
    let prefix = keep(&prepare(B), 1);
    assert_eq!(prefix.check_journal().choices.len(), 1);
    let state = prefix.into_state();
    assert_eq!(frame(&state).chooser(), C);
    let departure = apply(&state, B, GameCommand::Concede);
    assert!(departure.check_journal().choices.is_empty());
    assert!(departure.check_journal().moves.is_empty());
    assert_eq!(frame(departure.state()).chooser(), C);
    assert!(
        departure
            .state()
            .execution()
            .window()
            .cards()
            .current_identity(CardId(2))
            .is_none()
    );
    let finished = keep(departure.state(), 3);
    assert_eq!(finished.check_journal().moves.len(), 1);
    assert_eq!(zone(finished.state(), 1), ObjectZone::Battlefield);
    assert_eq!(zone(finished.state(), 4), ObjectZone::Graveyard(C));
}
#[test]
fn legend_departure_requires_exact_issued_frame_and_basis() {
    let state = prepare(A);
    assert_eq!(
        state
            .execution()
            .depart_players(&BTreeSet::from([B]), state.turn.as_ref(), None),
        Err(EffectError::UnsupportedDepartureState)
    );
    let foreign = frame(keep(&prepare(A), 1).state()).clone();
    for field in 0..3 {
        let mut bad = state.clone();
        match field {
            0 => bad.pending = None,
            1 => {
                bad.pending.as_mut().unwrap().frame =
                    RuleFrame::Checks(CheckDecision::Legend(foreign.clone()))
            }
            2 => bad.pending.as_mut().unwrap().basis.revision += 1,
            _ => unreachable!(),
        }
        let saved = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert_eq!(result, Err(GameError::UnsupportedConcessionScope));
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
}

#[test]
fn removing_only_legend_controller_leaves_no_fake_state_action_choice_or_movement() {
    let state = root(
        vec![legend_card(1, A, A, "Alpha"), legend_card(2, A, A, "Alpha")],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let departed = apply(&state, A, GameCommand::Concede);
    assert!(departed.check_journal().choices.is_empty());
    assert!(departed.check_journal().moves.is_empty());
    assert!(departed.state().pending_decision().is_none());
    assert_eq!(holder(departed.state()), B);
}

#[test]
fn exact_accepted_cohort_keeps_original_certificate_when_removed_prior_group_renumbers_current_conflict()
 {
    let state = root(
        vec![
            legend_card(1, B, A, "Alpha"),
            legend_card(2, B, A, "Alpha"),
            legend_card(5, A, A, "Beta"),
            legend_card(6, A, A, "Beta"),
            legend_card(3, C, C, "Gamma"),
            legend_card(4, C, C, "Gamma"),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let state = keep(&state, 5).into_state();
    assert_eq!(frame(&state).chooser(), A);
    let original = state.execution().qualification_legend_certificates();
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].0, original[0].1);
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(frame(&state).chooser(), C);
    let refreshed = state.execution().qualification_legend_certificates();
    assert_eq!(refreshed[0].1, original[0].1);
    assert_eq!(refreshed[0].2, original[0].2);
    assert_ne!(refreshed[0].0.id(), original[0].0.id());
    let state = apply(&state, D, GameCommand::Concede).into_state();
    assert_eq!(
        state.execution().qualification_legend_certificates(),
        refreshed
    );
    let finished = keep(&state, 3);
    assert_eq!(finished.check_journal().moves.len(), 2);
    assert_eq!(zone(finished.state(), 6), ObjectZone::Graveyard(A));
}
#[test]
fn departure_exiles_survivor_owned_legend_without_fabricating_its_rule_choice_or_graveyard_move() {
    let cards = vec![
        legend_card(1, A, A, "Alpha"),
        legend_card(2, A, A, "Alpha"),
        legend_card(9, C, B, "Beta"),
        legend_card(10, B, B, "Beta"),
        legend_card(3, C, C, "Gamma"),
        legend_card(4, C, C, "Gamma"),
    ];
    let state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
    let departed = apply(&state, B, GameCommand::Concede);
    assert!(departed.check_journal().moves.is_empty());
    assert!(departed.check_journal().choices.is_empty());
    assert_eq!(zone(departed.state(), 9), ObjectZone::Exile);
    let state = keep(departed.state(), 1).into_state();
    let finished = keep(&state, 3);
    assert_eq!(finished.check_journal().moves.len(), 2);
    assert_eq!(zone(finished.state(), 9), ObjectZone::Exile);
}

#[test]
fn legend_departure_real_exile_and_late_trigger_placement_failures_reject_without_partial_publication()
 {
    use crate::effects::private_qualification::QualificationFault;
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: OrdinaryCondition::LeavesBattlefield(
            crate::program::ObjectRelation::AnyCreature,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(7.into()),
            }],
        ),
    });
    let state = root(
        vec![
            legend_card(1, A, A, "Alpha"),
            legend_card(2, A, A, "Alpha"),
            legend_card(3, C, C, "Gamma"),
            legend_card(4, C, C, "Gamma"),
            card(
                9,
                C,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
            card(
                10,
                D,
                ObjectDestination::Battlefield { controller: D },
                watcher,
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    for fault in [
        QualificationFault::Revision(u64::MAX),
        QualificationFault::Event(u64::MAX),
        QualificationFault::ObjectIdentity(u64::MAX),
        QualificationFault::ObjectRevision(u64::MAX),
        QualificationFault::Instance(u64::MAX),
    ] {
        let mut bad = state.clone();
        bad.set_test_check_effect_fault(fault);
        let saved = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let expected = match fault {
            QualificationFault::Revision(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::RevisionExhausted,
            )),
            QualificationFault::Event(_) => GameError::Effect(EffectError::Trigger(
                crate::occurrence::TriggerError::EventIdentityExhausted,
            )),
            QualificationFault::Instance(_) => GameError::Effect(EffectError::Trigger(
                crate::occurrence::TriggerError::InstanceIdentityExhausted,
            )),
            QualificationFault::ObjectIdentity(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::Objects(
                    crate::object_identity::ObjectIdentityError::IdentityExhausted,
                ),
            )),
            QualificationFault::ObjectRevision(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::Objects(
                    crate::object_identity::ObjectIdentityError::RevisionExhausted,
                ),
            )),
            _ => unreachable!(),
        };
        let result = bad.advance(issued.clone());
        assert_eq!(result, Err(expected), "{fault:?}");
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
    for root_revision in [true, false] {
        let mut bad = state.clone();
        if root_revision {
            bad.set_test_input_revision(u64::MAX)
        } else {
            bad.set_test_next_decision(u64::MAX)
        }
        let saved = bad.clone();
        let result = bad.advance(input(&bad, B, GameCommand::Concede));
        assert_eq!(
            result,
            Err(if root_revision {
                GameError::RevisionExhausted
            } else {
                GameError::DecisionIdentityExhausted
            })
        );
        assert_eq!(bad, saved);
    }
    let state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(state.execution().waiting_triggers().count(), 1);
    let state = keep(&state, 1).into_state();
    for fault in [
        QualificationFault::Wave(u64::MAX),
        QualificationFault::StackEntry(u64::MAX),
    ] {
        let mut bad = state.clone();
        bad.set_test_check_effect_fault(fault);
        let saved = bad.clone();
        let pending = bad.pending_decision().unwrap();
        let conflict = frame(&bad).conflicts()[0].id();
        let issued = input(
            &bad,
            C,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::KeepLegend {
                    conflict,
                    keep: object(&bad, 3),
                },
            },
        );
        let result = bad.advance(issued.clone());
        let expected = match fault {
            QualificationFault::Wave(_) => GameError::Effect(EffectError::Trigger(
                crate::occurrence::TriggerError::WaveIdentityExhausted,
            )),
            QualificationFault::StackEntry(_) => GameError::Effect(EffectError::Window(
                crate::stack_window::StackWindowError::EntryIdentityExhausted,
            )),
            _ => unreachable!(),
        };
        assert_eq!(result, Err(expected));
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
    let finished = keep(&state, 3);
    assert_eq!(finished.check_journal().moves.len(), 2);
    assert_eq!(
        finished
            .rules_journal()
            .records
            .iter()
            .filter(|record| matches!(record, RulesRecord::TriggerPlaced(_)))
            .count(),
        1
    );
    let mut state = finished.into_state();
    for _ in 0..10 {
        if state.execution().window().entries().is_empty() {
            break;
        }
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().life_total(D), Some(&27.into()));
}

#[test]
fn original_legend_certificate_structural_faults_reject_valid_current_reply_and_departure_atomically()
 {
    use crate::effects::private_qualification::QualificationLegendFault as Fault;
    let state = keep(&prepare(A), 1).into_state();
    for fault in [
        Fault::Event,
        Fault::Controller,
        Fault::Domain,
        Fault::Names,
        Fault::Keep,
    ] {
        let mut bad = state.clone();
        bad.execution.qualification_corrupt_legend(fault);
        let saved = bad.clone();
        let pending = bad.pending_decision().unwrap();
        let RuleFrame::Checks(CheckDecision::Legend(frame)) = pending.frame() else {
            panic!()
        };
        for issued in [
            input(&bad, B, GameCommand::Concede),
            input(
                &bad,
                C,
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::KeepLegend {
                        conflict: frame.conflicts()[0].id(),
                        keep: object(&bad, 3),
                    },
                },
            ),
        ] {
            let result = bad.advance(issued.clone());
            assert_eq!(
                result,
                Err(GameError::Effect(EffectError::Check(
                    crate::checks::CheckError::WrongFrame
                ))),
                "{fault:?}"
            );
            assert_eq!(result, bad.advance(issued));
            assert_eq!(bad, saved);
        }
    }
}

#[test]
fn changed_legend_cohorts_receive_fresh_decisions_after_departure() {
    for merged in [false, true] {
        let cards = if merged {
            let mut cards = vec![];
            for (id, owner, names) in [
                (1, A, vec!["X", "Y"]),
                (2, A, vec!["X", "Y"]),
                (5, A, vec!["X", "Y"]),
                (6, B, vec!["X"]),
                (7, B, vec!["Y"]),
            ] {
                let mut card = legend_card(id, owner, A, "unused");
                card.program.names = ObjectNames::new(
                    names
                        .into_iter()
                        .map(|name| EnglishCardName::new(name.into()).unwrap())
                        .collect(),
                )
                .unwrap();
                cards.push(card);
            }
            cards.extend([legend_card(3, C, C, "Gamma"), legend_card(4, C, C, "Gamma")]);
            cards
        } else {
            vec![
                legend_card(1, B, A, "Alpha"),
                legend_card(2, A, A, "Alpha"),
                legend_card(3, C, C, "Gamma"),
                legend_card(4, C, C, "Gamma"),
            ]
        };
        let state = root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]);
        let mut state = keep(&state, 1).into_state();
        if merged {
            state = keep(&state, 2).into_state();
        }
        assert_eq!(frame(&state).chooser(), C);
        let saved = state.clone();
        let issued = input(&state, B, GameCommand::Concede);
        let result = state.advance(issued.clone());
        assert_eq!(result, state.advance(issued));
        let next = result.unwrap().into_state();
        assert_eq!(next.execution().window().participants(), &[A, C, D]);
        assert_eq!(frame(&next).chooser(), if merged { A } else { C });
        assert_eq!(state, saved);
    }
}

#[test]
fn actual_departed_control_effect_returns_same_legend_incarnation_without_committing_gone_chooser_action()
 {
    let control = card(
        10,
        A,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::SinglePermanent,
                vec![Instruction::CreateContinuousEffect {
                    parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Control(
                        ControlRecipient::ResolvingController,
                    )]),
                    duration: ResolvedDuration::NoStatedDuration,
                }],
            ),
        ),
    );
    let state = root(
        vec![
            legend_card(1, A, A, "Alpha"),
            legend_card(2, C, C, "Alpha"),
            control,
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let target = object(&state, 2);
    let state = cast_and_resolve(state, A, 10, target);
    assert_eq!(frame(&state).chooser(), A);
    let before = state
        .execution()
        .window()
        .cards()
        .get(target)
        .unwrap()
        .clone();
    assert_eq!(before.controller(), Some(A));
    let departed = apply(&state, A, GameCommand::Concede);
    let after = departed
        .state()
        .execution()
        .window()
        .cards()
        .get(target)
        .unwrap();
    assert_eq!(after.id(), before.id());
    assert_eq!(after.controller(), Some(C));
    assert_ne!(after.control_epoch(), before.control_epoch());
    assert!(departed.check_journal().moves.is_empty());
    assert!(departed.check_journal().choices.is_empty());
    assert!(departed.state().pending_decision().is_none());
    assert_eq!(holder(departed.state()), B);
}
#[test]
fn pending_loss_and_materialized_graveyard_order_continue_after_departure() {
    let zero_life = root(
        vec![legend_card(1, A, A, "Alpha"), legend_card(2, A, A, "Alpha")],
        &[20, 0, 20, 20],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    assert!(matches!(
        zero_life.pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::Legend(_))
    ));
    let state = root(
        vec![
            legend_card(1, A, A, "Alpha"),
            legend_card(2, A, A, "Alpha"),
            legend_card(3, A, A, "Alpha"),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let ordered = keep(&state, 1).into_state();
    assert!(matches!(
        ordered.pending_decision().unwrap().frame(),
        RuleFrame::Checks(CheckDecision::GraveyardOrder(_))
    ));
    for state in [zero_life, ordered] {
        let saved = state.clone();
        let issued = input(&state, D, GameCommand::Concede);
        let result = state.advance(issued.clone());
        assert_eq!(result, state.advance(issued));
        let next = result.unwrap().into_state();
        assert_eq!(next.execution().window().participants(), &[A, B, C]);
        assert_eq!(next.pending_decision().unwrap().chooser(), A);
        assert_eq!(state, saved);
    }
}

#[test]
fn real_name_effect_exposes_legend_choice_above_lower_spell_that_survives_both_departure_paths() {
    for leaver in [A, B] {
        let rename = card(
            10,
            A,
            ObjectDestination::Hand,
            Program::instant(
                ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Names(
                            crate::characteristics::NameOperation::Set(
                                ObjectNames::new(vec![
                                    EnglishCardName::new("Alpha".into()).unwrap(),
                                ])
                                .unwrap(),
                            ),
                        )]),
                        duration: ResolvedDuration::NoStatedDuration,
                    }],
                ),
            ),
        );
        let lower = card(
            20,
            D,
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
        );
        let mut state = root(
            vec![
                legend_card(1, A, A, "Alpha"),
                legend_card(2, A, A, "Beta"),
                rename,
                lower,
            ],
            &[20; 4],
            Some(Step::PrecombatMain),
            vec![A, B, C, D],
        );
        while holder(&state) != D {
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
        state = apply(
            &state,
            D,
            GameCommand::CastUntargeted {
                spell: object(&state, 20),
                payment: None,
            },
        )
        .into_state();
        let lower = state.execution().window().entries()[0].clone();
        while holder(&state) != A {
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
        state = apply(
            &state,
            A,
            GameCommand::Cast {
                spell: object(&state, 10),
                target: TargetRef::Object(object(&state, 2)),
            },
        )
        .into_state();
        for _ in 0..8 {
            if state.pending_decision().is_some() {
                break;
            }
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
        assert_eq!(frame(&state).chooser(), A);
        assert_eq!(
            state.execution().window().entries(),
            std::slice::from_ref(&lower)
        );
        let departed = apply(&state, leaver, GameCommand::Concede);
        state = if leaver == A {
            assert!(departed.check_journal().choices.is_empty());
            assert!(departed.check_journal().moves.is_empty());
            departed.into_state()
        } else {
            keep(departed.state(), 1).into_state()
        };
        assert_eq!(
            state.execution().window().entries(),
            std::slice::from_ref(&lower)
        );
        for _ in 0..8 {
            if state.execution().window().entries().is_empty() {
                break;
            }
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
        assert!(state.execution().window().entries().is_empty());
        assert_eq!(state.execution().life_total(D), Some(&27.into()));
    }
}

#[test]
fn departing_static_name_source_restores_distinct_names_and_preserves_the_other_conflict() {
    use crate::characteristics::{
        NameOperation, PermanentSelector, StaticAbilityId, StaticContinuousAbility,
        StaticNameOperand, StaticPart, StaticParts,
    };
    let mut source = Program::artifact(None, vec![]);
    source.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([crate::program::CardType::Artifact]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::Names(NameOperation::Set(
            StaticNameOperand::Literal(
                ObjectNames::new(vec![EnglishCardName::new("Shared".into()).unwrap()]).unwrap(),
            ),
        ))]),
    });
    let state = root(
        vec![
            legend_card(1, A, A, "Alpha"),
            legend_card(2, A, A, "Beta"),
            legend_card(3, C, C, "Gamma"),
            legend_card(4, C, C, "Gamma"),
            card(
                10,
                B,
                ObjectDestination::Battlefield { controller: B },
                source,
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let prefix = keep(&state, 1);
    assert_eq!(prefix.check_journal().choices.len(), 1);
    let state = prefix.into_state();
    assert_eq!(frame(&state).chooser(), C);
    let departure = apply(&state, B, GameCommand::Concede);
    assert!(departure.check_journal().choices.is_empty());
    assert!(departure.check_journal().moves.is_empty());
    assert_eq!(frame(departure.state()).chooser(), C);
    let finished = keep(departure.state(), 3);
    assert_eq!(finished.check_journal().moves.len(), 1);
    for id in [1, 2, 3] {
        assert_eq!(zone(finished.state(), id), ObjectZone::Battlefield);
    }
    assert_eq!(zone(finished.state(), 4), ObjectZone::Graveyard(C));
}
