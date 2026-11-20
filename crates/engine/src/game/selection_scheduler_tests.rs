//! Synthetic native composition: actual cleanup priority and repeated cleanup.
//! No source-card, full-format or ordinary observer qualification is implied.
use super::*;
use crate::{
    effects::{EffectError, ResolutionSelectionKind},
    life::LifeAdjustment,
    mana::ManaAbilityRef,
    program::{
        AffectedPlayers, CardType, InstructionAmount, ObjectRelation, PermanentTypeFilter,
        ResolvingPlayer, TriggerGrouping,
    },
};
use std::collections::BTreeSet;

#[path = "selection_scheduler_tests/expiry_and_tokens.rs"]
mod expiry_and_tokens;

struct SavedCall {
    input: GameInput,
    result: Result<NativeCandidate, GameError>,
}

fn recorded(
    state: &mut GameState,
    actor: SeatId,
    command: GameCommand,
    rows: &mut Vec<RulesRecord>,
    calls: &mut Vec<SavedCall>,
) -> NativeCandidate {
    let before = state.clone();
    let input = GameInput {
        basis: before.basis().clone(),
        actor,
        command: command.clone(),
    };
    let actual = accept(state, actor, command, rows);
    assert_eq!(before.advance(input.clone()), Ok(actual.clone()));
    calls.push(SavedCall {
        input,
        result: Ok(actual.clone()),
    });
    actual
}

fn recorded_reply(
    state: &mut GameState,
    choice: DecisionReply,
    rows: &mut Vec<RulesRecord>,
    calls: &mut Vec<SavedCall>,
) -> NativeCandidate {
    let decision = state.pending_decision().unwrap();
    recorded(
        state,
        decision.chooser(),
        GameCommand::Reply {
            decision: decision.id(),
            choice,
        },
        rows,
        calls,
    )
}

fn object(state: &GameState, card: u64) -> ObjectId {
    state
        .execution()
        .window()
        .cards()
        .current_identity(CardId(card))
        .unwrap()
}

fn assert_same_cleanup(state: &GameState) {
    let position = state.turn().unwrap().position();
    assert_eq!(position.number, 1);
    assert_eq!(position.step, Step::Cleanup);
    assert_eq!(position.cleanup_iteration, 0);
    assert_eq!(state.execution().mana(A, ManaType::Blue), 1.into());
}

fn replay_calls(initial: GameState, expected: &GameState, calls: Vec<SavedCall>) {
    let mut replayed = initial;
    for call in calls {
        let before = replayed.clone();
        let actual = replayed.advance(call.input);
        assert_eq!(replayed, before);
        assert_eq!(actual, call.result, "complete original typed-input replay");
        if let Ok(candidate) = actual {
            replayed = candidate.into_state();
        }
    }
    assert_eq!(&replayed, expected);
}

#[test]
fn selected_trigger_finishes_in_cleanup_priority_before_the_real_repeat() {
    // CR 514.3a: an actual beginning-of-cleanup trigger yields priority. Its
    // selected body must not execute another cleanup while it is suspended.
    let mut cards = libraries(&[A, B]);
    let mut source = observer(
        1,
        A,
        OrdinaryCondition::StepBegan {
            step: Step::Cleanup,
            player: PlayerRelation::Controller,
        },
    );
    source.program.triggered[0].body = ExecutableBody::new(
        BodyTarget::None,
        vec![
            Instruction::Discard {
                players: AffectedPlayers::EachPlayer,
                amount: InstructionAmount::Literal(2.into()),
            },
            Instruction::Sacrifice {
                players: AffectedPlayers::One(ResolvingPlayer::Controller),
                amount: InstructionAmount::Literal(1.into()),
                filter: PermanentTypeFilter {
                    required_types: BTreeSet::from([CardType::Artifact]),
                    forbidden_types: BTreeSet::new(),
                },
            },
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(1.into()),
            },
        ],
    );
    cards.push(source);
    cards.push(SeedCard {
        card: CardId(90),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: land(ManaType::Blue),
    });
    for (card, owner) in [(10, A), (11, A), (20, B), (21, B)] {
        cards.push(SeedCard {
            card: CardId(card),
            owner,
            destination: ObjectDestination::Hand,
            program: land(ManaType::White),
        });
    }
    let initial = scheduled(vec![A, B], A, Step::End, cards, vec![]);
    let initial_state = initial.state().clone();
    let libraries_before = [A, B].map(|p| {
        initial_state
            .execution()
            .window()
            .cards()
            .library(p)
            .to_vec()
    });
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    let mut calls = vec![];
    for actor in [A, B] {
        recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
    }
    assert_eq!(state.turn().unwrap().position().step, Step::Cleanup);
    assert_eq!(state.execution().window().entries().len(), 1);
    assert!(state.pending_decision().is_none());
    let entry = state.execution().window().entries()[0].id();
    let body = state.execution().stack_body(entry).unwrap().clone();
    assert_eq!(body.instructions.len(), 3);
    let mana_source = object(&state, 90);
    recorded(
        &mut state,
        A,
        GameCommand::ActivateMana {
            source: mana_source,
            ability: ManaAbilityRef::Printed(ManaAbilityId(1)),
        },
        &mut rows,
        &mut calls,
    );
    for actor in [A, B] {
        recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
    }
    let operation_start = rows.len();
    let before_cards = state.execution().window().cards().clone();
    let a = [object(&state, 10), object(&state, 11)];
    let b = [object(&state, 20), object(&state, 21)];
    for (player, selected) in [(A, a.to_vec()), (B, b.to_vec())] {
        assert_same_cleanup(&state);
        let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
            panic!("actual cleanup trigger selection")
        };
        assert_eq!(frame.chooser(), player);
        assert_eq!(frame.entry(), entry);
        assert_eq!(frame.instruction(), 0);
        assert_eq!(frame.operation(), ResolutionSelectionKind::Discard);
        assert_eq!(frame.required(), 2);
        assert_eq!(state.execution().window().cards(), &before_cards);
        recorded_reply(
            &mut state,
            DecisionReply::SelectResolution(selected),
            &mut rows,
            &mut calls,
        );
    }
    for (owner, order) in [(A, vec![a[1], a[0]]), (B, vec![b[1], b[0]])] {
        assert_same_cleanup(&state);
        let decision = state.pending_decision().unwrap();
        let RuleFrame::ResolutionGraveyardOrder(frame) = decision.frame() else {
            panic!("actual cleanup owner permutation")
        };
        assert_eq!(frame.chooser(), owner);
        assert_eq!(state.execution().window().cards(), &before_cards);
        if owner == B {
            let input = GameInput {
                basis: state.basis().clone(),
                actor: B,
                command: GameCommand::Reply {
                    decision: decision.id(),
                    choice: DecisionReply::OrderResolutionGraveyard(vec![]),
                },
            };
            let saved = state.clone();
            let result = state.advance(input.clone());
            assert_eq!(
                result,
                Err(GameError::Effect(EffectError::InvalidGraveyardPermutation))
            );
            assert_eq!(
                state, saved,
                "latest accepted first-owner prefix survives rejection"
            );
            calls.push(SavedCall { input, result });
        }
        recorded_reply(
            &mut state,
            DecisionReply::OrderResolutionGraveyard(order),
            &mut rows,
            &mut calls,
        );
    }
    assert_same_cleanup(&state);
    let source_before = object(&state, 1);
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("later sacrifice shares the trigger cursor")
    };
    assert_eq!(frame.instruction(), 1);
    assert_eq!(frame.domain(), &[source_before]);
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.operation(), ResolutionSelectionKind::Sacrifice);
    recorded_reply(
        &mut state,
        DecisionReply::SelectResolution(vec![source_before]),
        &mut rows,
        &mut calls,
    );
    assert_same_cleanup(&state);
    assert!(state.pending_decision().is_none());
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().life_total(A), Some(&21.into()));
    assert_eq!(state.execution().life_total(B), Some(&20.into()));
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 1))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    for (owner, expected) in [
        (A, vec![CardId(1), CardId(11), CardId(10)]),
        (B, vec![CardId(21), CardId(20)]),
    ] {
        let cards = state.execution().window().cards();
        assert_eq!(
            cards
                .graveyard(owner)
                .iter()
                .map(|id| cards.get(*id).unwrap().card_id().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
    }
    assert!(
        events(&rows[operation_start..])
            .iter()
            .all(|event| !matches!(
                event.cause,
                EventCause::StepBegan { .. }
                    | EventCause::CleanupCharacteristics { .. }
                    | EventCause::ManaPoolsEmptied { .. }
            ))
    );
    for actor in [A, B] {
        recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
    }
    let cleanups: Vec<_> = events(&rows)
        .into_iter()
        .filter_map(|event| match event.cause {
            EventCause::StepBegan { position } if position.step == Step::Cleanup => {
                Some((event.id, position.cleanup_iteration))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        cleanups.iter().map(|(_, n)| *n).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_ne!(cleanups[0].0, cleanups[1].0);
    assert_eq!(
        events(&rows)
            .iter()
            .filter(|event| matches!(event.cause, EventCause::ManaPoolsEmptied { .. }))
            .count(),
        1
    );
    assert_eq!(state.execution().mana(A, ManaType::Blue), 0.into());
    assert_eq!(state.turn().unwrap().position().number, 2);
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(state.execution().window().turn_owner(), B);
    for (owner, old) in [A, B].into_iter().zip(libraries_before) {
        assert_eq!(state.execution().window().cards().library(owner), &old);
    }
    replay_calls(initial_state, &state, calls);
}

#[test]
fn simultaneous_selected_deaths_capture_the_dying_observer_for_every_occurrence() {
    // CR603.6c/603.10a: the departing observer sees itself and the other
    // simultaneously sacrificed creature in the authentic before world.
    let trigger_body = ExecutableBody::new(
        BodyTarget::None,
        vec![Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(1.into()),
        }],
    );
    let mut watcher = Program::creature(None, 2.into(), 2.into(), BTreeSet::new());
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        grouping: TriggerGrouping::EachOccurrence,
        condition: OrdinaryCondition::Dies(ObjectRelation::AnyCreature),
        body: trigger_body.clone(),
    });
    let mut cards = libraries(&[A, B]);
    for (card, owner, program) in [
        (10, A, watcher),
        (
            20,
            B,
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
    ] {
        cards.push(SeedCard {
            card: CardId(card),
            owner,
            destination: ObjectDestination::Battlefield { controller: owner },
            program,
        });
    }
    cards.push(SeedCard {
        card: CardId(100),
        owner: A,
        destination: ObjectDestination::Hand,
        program: Program::instant(
            crate::mana::ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::Sacrifice {
                    players: AffectedPlayers::EachPlayer,
                    amount: InstructionAmount::Literal(1.into()),
                    filter: PermanentTypeFilter {
                        required_types: BTreeSet::from([CardType::Creature]),
                        forbidden_types: BTreeSet::new(),
                    },
                }],
            ),
        ),
    });
    let initial = scheduled(vec![A, B], A, Step::PrecombatMain, cards, vec![]);
    let initial_state = initial.state().clone();
    let mut state = initial.into_state();
    let mut rows = vec![];
    let mut calls = vec![];
    let spell = object(&state, 100);
    let before_watcher = object(&state, 10);
    let before_other = object(&state, 20);
    recorded(
        &mut state,
        A,
        GameCommand::CastUntargeted {
            spell,
            payment: None,
        },
        &mut rows,
        &mut calls,
    );
    let entry = state.execution().window().entries()[0].id();
    for actor in [A, B] {
        recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
    }
    let old_cards = state.execution().window().cards().clone();
    let first = recorded_reply(
        &mut state,
        DecisionReply::SelectResolution(vec![before_watcher]),
        &mut rows,
        &mut calls,
    );
    assert!(first.rules_journal().records.is_empty());
    assert_eq!(state.execution().window().cards(), &old_cards);
    assert_eq!(state.execution().waiting_triggers().count(), 0);
    let committed = recorded_reply(
        &mut state,
        DecisionReply::SelectResolution(vec![before_other]),
        &mut rows,
        &mut calls,
    );
    let simultaneous: Vec<_> = events(&committed.rules_journal().records)
        .into_iter()
        .filter(|event| {
            event.cause
                == EventCause::Instruction {
                    entry,
                    instruction: 0,
                }
        })
        .collect();
    let [event] = simultaneous.as_slice() else {
        panic!("one actual simultaneous sacrifice event")
    };
    let deaths: Vec<_> = event
        .occurrences
        .iter()
        .filter_map(|occurrence| match &occurrence.value {
            RulesOccurrence::ZoneChange(change)
                if matches!(change.reason, ZoneReason::Sacrifice { .. }) =>
            {
                Some((occurrence, change))
            }
            _ => None,
        })
        .collect();
    assert_eq!(deaths.len(), 2);
    assert_eq!(
        deaths
            .iter()
            .map(|(_, change)| change.before.object.id())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([before_watcher, before_other])
    );
    let discovered: Vec<_> = committed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some(waiting.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(discovered.len(), 2);
    assert_eq!(
        discovered
            .iter()
            .map(|w| w.id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    let mut matched_occurrences = BTreeSet::new();
    for waiting in &discovered {
        assert_eq!(waiting.source.object.id(), before_watcher);
        assert_eq!(waiting.source.object.zone(), ObjectZone::Battlefield);
        assert_eq!(waiting.controller, A);
        assert_eq!(waiting.body, trigger_body);
        assert_eq!(waiting.causes.event(), event.id);
        let [member] = waiting.causes.members() else {
            panic!("one real occurrence per trigger")
        };
        let (occurrence, movement) = deaths
            .iter()
            .find(|(occurrence, _)| occurrence.id == member.occurrence)
            .unwrap();
        assert!(matched_occurrences.insert(occurrence.id));
        assert_eq!(
            member.related,
            crate::occurrence::TriggerRelated::ZoneChange {
                before: movement.before.clone(),
                after: movement.after.clone(),
            }
        );
        assert_eq!(
            waiting.causes.key(),
            crate::occurrence::TriggerCauseKey::Occurrence(occurrence.id)
        );
        assert_eq!(waiting.definition, TriggeredAbilityId(1));
        assert_eq!(
            member.related_player,
            Some(movement.before.object.controller().unwrap())
        );
    }
    assert_eq!(matched_occurrences.len(), 2);
    assert!(
        state
            .execution()
            .window()
            .cards()
            .get(before_watcher)
            .is_none()
    );
    assert_ne!(object(&state, 10), before_watcher);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 10))
            .unwrap()
            .zone(),
        ObjectZone::Graveyard(A)
    );
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!("distinct later trigger placement order")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(
        frame.instances().iter().copied().collect::<BTreeSet<_>>(),
        discovered.iter().map(|w| w.id).collect()
    );
    assert!(state.execution().window().entries().is_empty());
    let ordered: Vec<_> = frame.instances().iter().rev().copied().collect();
    let placed = recorded_reply(
        &mut state,
        DecisionReply::OrderTriggers(ordered.clone()),
        &mut rows,
        &mut calls,
    );
    let placements: Vec<_> = placed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerPlaced(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(placements.len(), 2);
    assert_eq!(
        placements.iter().map(|p| p.instance).collect::<Vec<_>>(),
        ordered
    );
    assert_eq!(
        placements.iter().map(|p| p.position).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(
        placements
            .iter()
            .all(|p| p.controller == A && p.wave == placements[0].wave)
    );
    assert_eq!(
        placements
            .iter()
            .map(|p| p.entry)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        state
            .execution()
            .window()
            .entries()
            .iter()
            .map(|e| e.id())
            .collect::<Vec<_>>(),
        placements.iter().map(|p| p.entry).collect::<Vec<_>>()
    );
    for placement in placements {
        let capture = discovered
            .iter()
            .find(|waiting| waiting.id == placement.instance)
            .unwrap();
        let binding = state
            .execution()
            .execution_bindings(placement.entry)
            .unwrap()
            .trigger
            .as_ref()
            .unwrap();
        assert_eq!(binding.instance, capture.id);
        assert_eq!(binding.definition, capture.definition);
        assert_eq!(binding.source, capture.source);
        assert_eq!(binding.causes, capture.causes);
        assert_eq!(
            state.execution().stack_body(placement.entry),
            Some(&capture.body)
        );
    }
    assert!(state.pending_decision().is_none());
    for _ in 0..2 {
        for actor in [A, B] {
            recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
        }
    }
    assert_eq!(state.execution().life_total(A), Some(&22.into()));
    assert_eq!(state.execution().life_total(B), Some(&20.into()));
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().waiting_triggers().count(), 0);
    assert_eq!(state.turn().unwrap().position().step, Step::PrecombatMain);
    replay_calls(initial_state, &state, calls);
}

#[test]
fn actual_cleanup_discard_does_not_enter_effect_only_replacement_work() {
    use crate::program::{
        PublicZoneDestination, ZoneActionKind, ZoneActionPredicate, ZoneObjectKind,
        ZoneReplacementAbility, ZoneReplacementAbilityId, ZoneReplacementChoice,
        ZoneReplacementSubject,
    };
    let mut cards = libraries(&[A, B]);
    let mut replacement = Program::enchantment(None);
    replacement.zone_replacements.push(ZoneReplacementAbility {
        id: ZoneReplacementAbilityId(1),
        actions: ZoneActionPredicate::OneOf(BTreeSet::from([ZoneActionKind::EffectDiscard])),
        objects: ZoneObjectKind::Card,
        subject: ZoneReplacementSubject::AnyObject,
        destination: PublicZoneDestination::Graveyard,
        replace_with: PublicZoneDestination::Exile,
        choice: ZoneReplacementChoice::OptionalAffectedPlayer,
    });
    cards.push(SeedCard {
        card: CardId(90),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: replacement,
    });
    for card in 10..18 {
        cards.push(SeedCard {
            card: CardId(card),
            owner: A,
            destination: ObjectDestination::Hand,
            program: land(ManaType::White),
        });
    }
    let initialized = scheduled(vec![A, B], A, Step::End, cards, vec![]);
    let initial = initialized.state().clone();
    let mut state = initial.clone();
    let mut rows = initialized.rules_journal().records.clone();
    let mut calls = vec![];
    for actor in [A, B] {
        recorded(&mut state, actor, GameCommand::Pass, &mut rows, &mut calls);
    }
    let RuleFrame::Checks(crate::checks::CheckDecision::CleanupDiscard(frame)) =
        state.pending_decision().unwrap().frame()
    else {
        panic!("actual cleanup discard")
    };
    assert_eq!(frame.required(), 1);
    let selected = object(&state, 10);
    let result = recorded_reply(
        &mut state,
        DecisionReply::Discard(vec![selected]),
        &mut rows,
        &mut calls,
    );
    let moved = state
        .execution()
        .window()
        .cards()
        .get(object(&state, 10))
        .unwrap();
    assert_eq!(
        moved.zone(),
        crate::object_identity::ObjectZone::Graveyard(A)
    );
    assert!(!matches!(
        state.pending_decision().map(|d| d.frame()),
        Some(RuleFrame::ZoneReplacement(_))
    ));
    assert!(result.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::Event(event) if event.occurrences.iter().any(|row|matches!(&row.value,crate::occurrence::RulesOccurrence::ZoneChange(change) if change.before.object.id()==selected && change.reason==crate::occurrence::ZoneReason::Discard {player:A})))));
    replay_calls(initial, &state, calls);
}
