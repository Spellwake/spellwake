//! Synthetic native compositions through real casts and scheduled turns.
//! Preparation only; these fixtures are not source9 card or LKI qualification.
use super::*;
use crate::{
    characteristics::{
        ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec, SubtypeOperation, TypeChangeSpec,
        TypeOperation,
    },
    effects::OptionalEffectChoice,
    occurrence::{EntryCompletion, TriggerCauseKey, TriggerRelated},
    stack_window::{StackKind, WindowPhase},
    targets::TargetRef,
    token::{TokenCreation, TokenDefinition, TokenName},
};
use spellwake_contracts::names::EnglishCardName;

fn recorded_pass_round(
    state: &mut GameState,
    rows: &mut Vec<RulesRecord>,
    calls: &mut Vec<SavedCall>,
) -> NativeCandidate {
    let mut last = None;
    for _ in 0..state.execution().window().participants().len() {
        assert!(state.pending_decision().is_none());
        let WindowPhase::Priority(crate::priority::PriorityPhase::Ready { holder }) =
            state.execution().window().phase()
        else {
            panic!("actual later priority pass required")
        };
        last = Some(recorded(state, holder, GameCommand::Pass, rows, calls));
    }
    last.unwrap()
}

#[test]
fn real_until_end_of_turn_expiry_changes_the_cleanup_selection_domain_before_the_repeat() {
    let selected_body = ExecutableBody::new(
        BodyTarget::None,
        vec![
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
    let mut source = observer(
        1,
        A,
        OrdinaryCondition::StepBegan {
            step: Step::Cleanup,
            player: PlayerRelation::Controller,
        },
    );
    source.program.triggered[0].body = selected_body.clone();
    let mut cards = libraries(&[A, B]);
    cards.extend([
        source,
        SeedCard {
            card: CardId(90),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: land(ManaType::Blue),
        },
        SeedCard {
            card: CardId(200),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                crate::mana::ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::SinglePermanent,
                    vec![Instruction::CreateContinuousEffect {
                        parts: ResolvedPartsSpec(vec![ResolvedPartSpec::Type(TypeChangeSpec {
                            land_subtypes: SubtypeOperation::Keep,
                            supertypes: TypeOperation::Keep,
                            types: TypeOperation::Add(BTreeSet::from([CardType::Artifact])),
                            subtypes: SubtypeOperation::Keep,
                        })]),
                        duration: ResolvedDuration::UntilEndOfTurn,
                    }],
                ),
            ),
        },
    ]);
    let initial = scheduled(vec![A, B], A, Step::End, cards, vec![]);
    let initial_state = initial.state().clone();
    let old_libraries = [A, B].map(|player| {
        initial_state
            .execution()
            .window()
            .cards()
            .library(player)
            .to_vec()
    });
    let mut rows = initial.rules_journal().records.clone();
    let mut state = initial.into_state();
    let mut calls = vec![];
    let land_before = object(&state, 90);
    let changing_spell = object(&state, 200);
    recorded(
        &mut state,
        A,
        GameCommand::Cast {
            spell: changing_spell,
            target: TargetRef::Object(land_before),
        },
        &mut rows,
        &mut calls,
    );
    let effect_entry = state.execution().window().entries()[0].id();
    let resolved = recorded_pass_round(&mut state, &mut rows, &mut calls);
    let effects: Vec<_> = events(&resolved.rules_journal().records)
        .into_iter()
        .filter(|event| {
            event.cause
                == EventCause::Instruction {
                    entry: effect_entry,
                    instruction: 0,
                }
        })
        .flat_map(|event| &event.occurrences)
        .filter_map(|row| match &row.value {
            RulesOccurrence::ContinuousEffectCreated { effect, .. } => Some(effect),
            _ => None,
        })
        .collect();
    let [effect] = effects.as_slice() else {
        panic!("one actually cast temporary type effect")
    };
    let effect_id = effect.id;
    assert_eq!(effect.creator, effect_entry);
    assert_eq!(effect.duration, ResolvedDuration::UntilEndOfTurn);
    assert_eq!(effect.affected, BTreeSet::from([land_before]));
    assert_eq!(state.turn().unwrap().position().step, Step::End);
    assert_eq!(
        state
            .execution()
            .current_characteristics(land_before)
            .unwrap()
            .unwrap()
            .current_types,
        BTreeSet::from([CardType::Land, CardType::Artifact])
    );
    assert_eq!(state.execution().continuous_effects().count(), 1);

    let entered = recorded_pass_round(&mut state, &mut rows, &mut calls);
    let cleanup = state.turn().unwrap().position();
    assert_eq!(cleanup.number, 1);
    assert_eq!(cleanup.step, Step::Cleanup);
    assert_eq!(cleanup.cleanup_iteration, 0);
    let expiry_events: Vec<_> = events(&entered.rules_journal().records)
        .into_iter()
        .filter(|event| event.cause == EventCause::CleanupCharacteristics { position: cleanup })
        .collect();
    let [expiry] = expiry_events.as_slice() else {
        panic!("actual scheduled cleanup must expire the real effect")
    };
    let [expired] = expiry.occurrences.as_slice() else {
        panic!("one cleanup characteristics occurrence")
    };
    let RulesOccurrence::CleanupCharacteristics {
        expired_effects,
        marks_removed,
        appearances,
    } = &expired.value
    else {
        panic!("real expiration receipt")
    };
    assert_eq!(expired_effects.as_slice(), &[effect_id]);
    assert!(marks_removed.is_empty());
    let [appearance] = appearances.as_slice() else {
        panic!("only the temporary artifact land changes appearance")
    };
    assert_eq!(appearance.object, land_before);
    assert_eq!(
        appearance.before.types,
        BTreeSet::from([CardType::Land, CardType::Artifact])
    );
    assert_eq!(appearance.after.types, BTreeSet::from([CardType::Land]));
    assert_eq!(state.execution().continuous_effects().count(), 0);
    assert_eq!(object(&state, 90), land_before);
    assert!(state.pending_decision().is_none());
    let [trigger] = state.execution().window().entries() else {
        panic!("one real cleanup-entry trigger yields exception priority")
    };
    let trigger_entry = trigger.id();
    assert_eq!(trigger.controller(), A);
    assert_eq!(
        state.execution().stack_body(trigger_entry),
        Some(&selected_body)
    );

    recorded_pass_round(&mut state, &mut rows, &mut calls);
    assert_eq!(state.turn().unwrap().position(), cleanup);
    let source_before = object(&state, 1);
    let RuleFrame::ResolutionSelect(frame) = state.pending_decision().unwrap().frame() else {
        panic!("cleanup body suspends on the real forced selection")
    };
    assert_eq!(frame.entry(), trigger_entry);
    assert_eq!(frame.instruction(), 0);
    assert_eq!(frame.chooser(), A);
    assert_eq!(frame.operation(), ResolutionSelectionKind::Sacrifice);
    assert_eq!(frame.required(), 1);
    // Before expiry, land90 was another Artifact. The selection now proves that
    // actual expiry, rather than merely an ambient cleanup event, changed eligibility.
    assert_eq!(frame.domain(), &[source_before]);
    assert!(!frame.domain().contains(&land_before));
    recorded_reply(
        &mut state,
        DecisionReply::SelectResolution(vec![source_before]),
        &mut rows,
        &mut calls,
    );
    assert_eq!(state.turn().unwrap().position(), cleanup);
    assert!(state.pending_decision().is_none());
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().life_total(A), Some(&21.into()));
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
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(land_before)
            .unwrap()
            .zone(),
        ObjectZone::Battlefield
    );
    assert_eq!(
        events(&rows)
            .iter()
            .filter(|event| matches!(event.cause,
                EventCause::StepBegan { position } if position.step == Step::Cleanup
            ))
            .count(),
        1,
        "no repeated cleanup while resolving or suspended"
    );

    recorded_pass_round(&mut state, &mut rows, &mut calls);
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
        cleanups
            .iter()
            .map(|(_, iteration)| *iteration)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_ne!(cleanups[0].0, cleanups[1].0);
    assert_eq!(
        events(&rows)
            .iter()
            .filter(|event| matches!(event.cause, EventCause::CleanupCharacteristics { .. }))
            .count(),
        1,
        "the same effect must not expire twice"
    );
    assert_eq!(state.turn().unwrap().position().number, 2);
    assert_eq!(state.turn().unwrap().position().step, Step::Upkeep);
    assert_eq!(state.execution().window().turn_owner(), B);
    for (player, original_library) in [A, B].into_iter().zip(old_libraries) {
        assert_eq!(
            state.execution().window().cards().library(player),
            &original_library
        );
    }
    replay_calls(initial_state, &state, calls);
}

#[test]
fn a_real_created_token_and_dying_observer_share_lookback_before_later_cessation_and_placement() {
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
    let vocabulary =
        crate::test_support::creature_types::synthetic_vocabulary_for_rules("cr-20260925");
    let token = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Specified(EnglishCardName::new("Synthetic Lookback Token".into()).unwrap()),
        &vocabulary,
    )
    .unwrap();
    let mut cards = libraries(&[A, B]);
    cards.extend([
        SeedCard {
            card: CardId(10),
            owner: A,
            destination: ObjectDestination::Battlefield { controller: A },
            program: watcher,
        },
        SeedCard {
            card: CardId(200),
            owner: B,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                crate::mana::ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![Instruction::CreateTokens {
                        creations: vec![TokenCreation::one(token)],
                    }],
                ),
            ),
        },
        SeedCard {
            card: CardId(100),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::instant(
                crate::mana::ManaCost::free(),
                ExecutableBody::new(
                    BodyTarget::None,
                    vec![
                        Instruction::Sacrifice {
                            players: AffectedPlayers::EachPlayer,
                            amount: InstructionAmount::Literal(1.into()),
                            filter: PermanentTypeFilter {
                                required_types: BTreeSet::from([CardType::Creature]),
                                forbidden_types: BTreeSet::new(),
                            },
                        },
                        // A real resolution decision exposes the gap between movement
                        // and later checks without injecting an SBA or private state.
                        Instruction::OptionalEffect {
                            chooser: ResolvingPlayer::Controller,
                        },
                        Instruction::AdjustControllerLife {
                            adjustment: LifeAdjustment::Gain(1.into()),
                        },
                    ],
                ),
            ),
        },
    ]);
    let initial = scheduled(vec![A, B], A, Step::PrecombatMain, cards, vec![]);
    let initial_state = initial.state().clone();
    let mut state = initial.into_state();
    let mut rows = vec![];
    let mut calls = vec![];
    assert!(
        state
            .execution()
            .window()
            .cards()
            .iter()
            .all(|object| !object.is_token())
    );
    recorded(&mut state, A, GameCommand::Pass, &mut rows, &mut calls);
    let token_spell = object(&state, 200);
    recorded(
        &mut state,
        B,
        GameCommand::CastUntargeted {
            spell: token_spell,
            payment: None,
        },
        &mut rows,
        &mut calls,
    );
    let creation_entry = state.execution().window().entries()[0].id();
    let created = recorded_pass_round(&mut state, &mut rows, &mut calls);
    let births: Vec<_> = events(&created.rules_journal().records)
        .into_iter()
        .filter(|event| {
            event.cause
                == EventCause::Instruction {
                    entry: creation_entry,
                    instruction: 0,
                }
        })
        .flat_map(|event| &event.occurrences)
        .filter_map(|row| match &row.value {
            RulesOccurrence::TokenCreated { creator, after } => Some((*creator, after.as_ref())),
            _ => None,
        })
        .collect();
    let [(creator, birth)] = births.as_slice() else {
        panic!("one actual native token birth")
    };
    assert_eq!(*creator, B);
    assert!(birth.object.is_token());
    assert!(birth.object.card_id().is_none());
    assert_eq!(birth.object.controller(), Some(B));
    assert_eq!(birth.object.zone(), ObjectZone::Battlefield);
    let token_before = birth.object.id();
    assert_eq!(
        state.execution().window().cards().get(token_before),
        Some(&birth.object)
    );
    assert!(state.pending_decision().is_none());
    assert_eq!(state.execution().waiting_triggers().count(), 0);
    let watcher_before = object(&state, 10);
    let selecting_spell = object(&state, 100);
    recorded(
        &mut state,
        A,
        GameCommand::CastUntargeted {
            spell: selecting_spell,
            payment: None,
        },
        &mut rows,
        &mut calls,
    );
    let entry = state.execution().window().entries()[0].id();
    recorded_pass_round(&mut state, &mut rows, &mut calls);
    let before_cards = state.execution().window().cards().clone();
    let RuleFrame::ResolutionSelect(first_frame) = state.pending_decision().unwrap().frame() else {
        panic!("A's actual forced sacrifice")
    };
    assert_eq!(first_frame.chooser(), A);
    assert_eq!(first_frame.domain(), &[watcher_before]);
    let first = recorded_reply(
        &mut state,
        DecisionReply::SelectResolution(vec![watcher_before]),
        &mut rows,
        &mut calls,
    );
    assert!(first.rules_journal().records.is_empty());
    assert_eq!(state.execution().window().cards(), &before_cards);
    let RuleFrame::ResolutionSelect(second_frame) = state.pending_decision().unwrap().frame()
    else {
        panic!("B's actual token sacrifice")
    };
    assert_eq!(second_frame.chooser(), B);
    assert_eq!(second_frame.domain(), &[token_before]);
    let committed = recorded_reply(
        &mut state,
        DecisionReply::SelectResolution(vec![token_before]),
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
        panic!("one actual simultaneous token/observer sacrifice event")
    };
    let deaths: Vec<_> = event
        .occurrences
        .iter()
        .filter_map(|row| match &row.value {
            RulesOccurrence::ZoneChange(change)
                if matches!(change.reason, ZoneReason::Sacrifice { .. }) =>
            {
                Some((row, change))
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
        BTreeSet::from([watcher_before, token_before])
    );
    let watcher_change = deaths
        .iter()
        .find(|(_, change)| change.before.object.id() == watcher_before)
        .unwrap()
        .1;
    let token_change = deaths
        .iter()
        .find(|(_, change)| change.before.object.id() == token_before)
        .unwrap()
        .1;
    assert_eq!(watcher_change.reason, ZoneReason::Sacrifice { player: A });
    assert_eq!(token_change.reason, ZoneReason::Sacrifice { player: B });
    assert_eq!(token_change.before, **birth);
    assert_eq!(token_change.after.object.zone(), ObjectZone::Graveyard(B));
    assert!(token_change.after.object.card_id().is_none());
    let departed_token = token_change.after.object.clone();
    assert_ne!(departed_token.id(), token_before);
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
            .map(|waiting| waiting.id)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    let mut matched = BTreeSet::new();
    for waiting in &discovered {
        assert_eq!(waiting.source, watcher_change.before);
        assert_eq!(waiting.controller, A);
        assert_eq!(waiting.definition, TriggeredAbilityId(1));
        assert_eq!(waiting.body, trigger_body);
        assert_eq!(waiting.causes.event(), event.id);
        let [member] = waiting.causes.members() else {
            panic!("one authentic death per trigger")
        };
        let (occurrence, movement) = deaths
            .iter()
            .find(|(occurrence, _)| occurrence.id == member.occurrence)
            .unwrap();
        assert!(matched.insert(occurrence.id));
        assert_eq!(
            waiting.causes.key(),
            TriggerCauseKey::Occurrence(occurrence.id)
        );
        assert_eq!(member.related_player, movement.before.object.controller());
        assert_eq!(
            member.related,
            TriggerRelated::ZoneChange {
                before: movement.before.clone(),
                after: movement.after.clone()
            }
        );
    }
    assert_eq!(
        matched,
        deaths.iter().map(|(occurrence, _)| occurrence.id).collect()
    );
    let RuleFrame::OptionalEffect(hold) = state.pending_decision().unwrap().frame() else {
        panic!("actual later clause retains resolution before SBA")
    };
    assert_eq!(hold.entry(), entry);
    assert_eq!(hold.instruction(), 1);
    assert_eq!(hold.chooser(), A);
    assert_eq!(state.execution().waiting_triggers().count(), 2);
    assert_eq!(state.execution().window().entries().len(), 1);
    assert_eq!(state.execution().window().entries()[0].id(), entry);
    assert_eq!(
        state.execution().window().cards().get(departed_token.id()),
        Some(&departed_token)
    );
    assert!(
        events(&committed.rules_journal().records)
            .iter()
            .flat_map(|event| &event.occurrences)
            .all(|row| !matches!(row.value, RulesOccurrence::TokenCeased { .. }))
    );
    assert!(
        committed
            .rules_journal()
            .records
            .iter()
            .all(|record| !matches!(record, RulesRecord::TriggerPlaced(_)))
    );
    let frozen = state.clone();
    let input = GameInput {
        basis: state.basis().clone(),
        actor: A,
        command: GameCommand::Pass,
    };
    let result = state.advance(input.clone());
    assert_eq!(result, Err(GameError::DecisionRequired));
    assert_eq!(state, frozen);
    calls.push(SavedCall { input, result });

    let finished = recorded_reply(
        &mut state,
        DecisionReply::OptionalEffect(OptionalEffectChoice::Decline),
        &mut rows,
        &mut calls,
    );
    let cessations: Vec<_> = events(&finished.rules_journal().records)
        .into_iter()
        .flat_map(|event| &event.occurrences)
        .filter_map(|row| match &row.value {
            RulesOccurrence::TokenCeased { object } => Some(object),
            _ => None,
        })
        .collect();
    assert_eq!(cessations, vec![&departed_token]);
    assert!(
        state
            .execution()
            .window()
            .cards()
            .get(departed_token.id())
            .is_none()
    );
    assert_eq!(state.execution().life_total(A), Some(&20.into()));
    let move_position = rows
        .iter()
        .position(|record| matches!(record, RulesRecord::Event(actual) if actual.id == event.id))
        .unwrap();
    let cessation_position = rows.iter().position(|record| matches!(record, RulesRecord::Event(actual) if actual.occurrences.iter().any(|row| matches!(&row.value,
        RulesOccurrence::TokenCeased { object } if object == &departed_token
    )))).unwrap();
    assert!(
        move_position < cessation_position,
        "distinct later actual token-cessation event"
    );
    let RuleFrame::Triggers(frame) = state.pending_decision().unwrap().frame() else {
        panic!("placement waits until resolution and later checks finish")
    };
    assert_eq!(frame.chooser(), A);
    assert_eq!(
        frame.instances().iter().copied().collect::<BTreeSet<_>>(),
        discovered.iter().map(|waiting| waiting.id).collect()
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
            RulesRecord::TriggerPlaced(placement) => Some(placement),
            _ => None,
        })
        .collect();
    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.instance)
            .collect::<Vec<_>>(),
        ordered
    );
    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.position)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.entry)
            .collect::<BTreeSet<_>>()
            .len(),
        2
    );
    assert!(
        placements
            .iter()
            .all(|placement| placement.controller == A && placement.wave == placements[0].wave)
    );
    assert_eq!(
        state
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<Vec<_>>(),
        placements
            .iter()
            .map(|placement| placement.entry)
            .collect::<Vec<_>>()
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
    for gained in 1..=2 {
        let expected_entry = state.execution().window().entries().last().unwrap().id();
        let resolved = recorded_pass_round(&mut state, &mut rows, &mut calls);
        let life: Vec<_> = events(&resolved.rules_journal().records)
            .into_iter()
            .filter(|event| {
                event.cause
                    == EventCause::Instruction {
                        entry: expected_entry,
                        instruction: 0,
                    }
            })
            .flat_map(|event| &event.occurrences)
            .filter_map(|row| match &row.value {
                RulesOccurrence::LifeChanged(change) => Some(change),
                _ => None,
            })
            .collect();
        let [change] = life.as_slice() else {
            panic!("one actual gain from the exact placed trigger")
        };
        assert_eq!(change.player, A);
        assert_eq!(change.amount, 1.into());
        assert_eq!(events(&resolved.rules_journal().records).iter().flat_map(|event| &event.occurrences).filter(|row| matches!(&row.value,
            RulesOccurrence::EntryCompleted { entry, controller, kind: StackKind::TriggeredAbility { source }, completion: EntryCompletion::Resolved }
                if *entry == expected_entry && *controller == A && source.id() == watcher_before
        )).count(), 1);
        assert_eq!(state.execution().life_total(A), Some(&(20 + gained).into()));
    }
    assert_eq!(state.execution().life_total(B), Some(&20.into()));
    assert!(state.execution().window().entries().is_empty());
    assert_eq!(state.execution().waiting_triggers().count(), 0);
    assert_eq!(state.turn().unwrap().position().step, Step::PrecombatMain);
    let cards = state.execution().window().cards();
    assert!(cards.iter().all(|object| !object.is_token()));
    assert_eq!(
        cards
            .graveyard(A)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(100), CardId(10)]
    );
    assert_eq!(
        cards
            .graveyard(B)
            .iter()
            .map(|id| cards.get(*id).unwrap().card_id().unwrap())
            .collect::<Vec<_>>(),
        vec![CardId(200)]
    );
    replay_calls(initial_state, &state, calls);
}
