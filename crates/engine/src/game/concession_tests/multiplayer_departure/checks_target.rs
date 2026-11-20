//! Actual provisional trigger placement survives or ceases through departure.
use super::*;
use crate::effects::CopyTargetChoice;
use crate::occurrence::{TriggerPlacement, TriggerRemovalReason};
use crate::program::{BodyTarget, SpellTargetFilter};
const D: SeatId = SeatId(77);
fn mana_watcher(target: BodyTarget, count: u64) -> Program {
    let mut source = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: crate::mana::ManaAbilityId(7),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 1.into())])),
        }],
    );
    for id in 1..=count {
        let instruction = match target {
            BodyTarget::SingleCreature | BodyTarget::SinglePermanent => {
                Instruction::ReturnToOwnersHand
            }
            BodyTarget::SingleCreatureOrPlayer => Instruction::Damage { amount: 2.into() },
            BodyTarget::SinglePlayer => Instruction::AdjustTargetLife {
                adjustment: LifeAdjustment::Gain(i64::try_from(id + 1).unwrap().into()),
            },
            BodyTarget::SingleSpell(_) => Instruction::CounterSpell,
            _ => panic!("finite target fixture"),
        };
        source.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: OrdinaryCondition::BecomesTapped(crate::program::ObjectRelation::SelfObject),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(target, vec![instruction]),
        });
    }
    source
}
fn activate(mut state: GameState) -> GameState {
    state = apply(
        &state,
        A,
        GameCommand::ActivateMana {
            source: object(&state, 1),
            ability: ManaAbilityRef::Printed(crate::mana::ManaAbilityId(7)),
        },
    )
    .into_state();
    if let Some(pending) = state.pending_decision()
        && let RuleFrame::Triggers(frame) = pending.frame()
    {
        state = apply(
            &state,
            A,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::OrderTriggers(frame.instances().to_vec()),
            },
        )
        .into_state();
    }
    assert!(matches!(
        state.pending_decision().unwrap().frame(),
        RuleFrame::TriggerTarget(_)
    ));
    state
}
fn prepared(target: BodyTarget, count: u64) -> GameState {
    activate(root(
        vec![
            card(
                9,
                C,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
            card(
                1,
                B,
                ObjectDestination::Battlefield { controller: A },
                mana_watcher(target, count),
            ),
            card(
                20,
                B,
                ObjectDestination::Battlefield { controller: B },
                creature(),
            ),
            card(
                30,
                D,
                ObjectDestination::Battlefield { controller: D },
                creature(),
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    ))
}
fn metadata(state: &GameState) -> TriggerPlacement {
    state
        .execution()
        .pending_trigger_placement()
        .unwrap()
        .unwrap()
}
fn choose(state: &GameState, target: TargetRef) -> NativeCandidate {
    let pending = state.pending_decision().unwrap();
    apply(
        state,
        pending.chooser(),
        GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::TriggerTarget(target),
        },
    )
}
fn settle(mut state: GameState) -> GameState {
    for _ in 0..100 {
        if state.execution().window().entries().is_empty() {
            return state;
        }
        if let Some(pending) = state.pending_decision() {
            assert!(matches!(pending.frame(), RuleFrame::CopyTargets(_)));
            state = apply(
                &state,
                pending.chooser(),
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
                },
            )
            .into_state();
        } else {
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
    }
    panic!("bounded genuine stack continuation")
}
fn removed(candidate: &NativeCandidate) -> Vec<(TriggerPlacement, TriggerRemovalReason)> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerRemoved { placement, reason } => Some((placement.clone(), *reason)),
            _ => None,
        })
        .collect()
}
#[test]
fn source_owner_and_object_target_departure_refreshes_one_remaining_target_without_new_entry() {
    let state = prepared(BodyTarget::SingleCreature, 1);
    let old = state.pending_decision().unwrap().clone();
    let before = metadata(&state);
    let capture = state.execution().waiting_triggers().next().unwrap().clone();
    let next_entry = state.execution().window().next_entry_sequence();
    let departed = apply(&state, B, GameCommand::Concede);
    assert!(removed(&departed).is_empty());
    let state = departed.state();
    assert!(
        state
            .execution()
            .window()
            .cards()
            .current_identity(CardId(1))
            .is_none()
    );
    assert_eq!(metadata(state), before);
    assert_eq!(state.execution().window().next_entry_sequence(), next_entry);
    assert_eq!(
        state
            .execution()
            .waiting_triggers()
            .cloned()
            .collect::<Vec<_>>(),
        vec![capture]
    );
    let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.domain(), &[TargetRef::Object(object(state, 30))]);
    assert_ne!(state.pending_decision().unwrap().id(), old.id());
    assert_eq!(
        state.advance(input(
            state,
            A,
            GameCommand::Reply {
                decision: old.id(),
                choice: DecisionReply::TriggerTarget(TargetRef::Object(object(state, 30)))
            }
        )),
        Err(GameError::WrongDecision)
    );
    let chosen = choose(state, TargetRef::Object(object(state, 30)));
    assert!(chosen.rules_journal().records.iter().any(|record|matches!(record,RulesRecord::TriggerTargetSelected {placement,..} if placement==&before)));
    assert!(chosen.rules_journal().records.iter().any(
        |record| matches!(record,RulesRecord::TriggerPlaced(placement) if placement==&before)
    ));
    let state = settle(chosen.into_state());
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(object(&state, 30))
            .unwrap()
            .zone(),
        crate::object_identity::ObjectZone::Hand(D)
    );
}
#[test]
fn total_object_target_loss_removes_only_actual_provisional_entry_once_without_fake_reply() {
    let state = prepared(BodyTarget::SingleCreature, 1);
    let initial = metadata(&state);
    let next = state.execution().window().next_entry_sequence();
    let state = apply(&state, B, GameCommand::Concede).into_state();
    let departed = apply(&state, D, GameCommand::Concede);
    assert_eq!(
        removed(&departed),
        vec![(initial, TriggerRemovalReason::NoLegalTargets)]
    );
    assert!(
        departed
            .rules_journal()
            .records
            .iter()
            .all(|record| !matches!(
                record,
                RulesRecord::TriggerTargetSelected { .. } | RulesRecord::TriggerPlaced(_)
            ))
    );
    assert!(departed.state().pending_decision().is_none());
    assert!(
        departed
            .state()
            .execution()
            .waiting_triggers()
            .next()
            .is_none()
    );
    assert!(departed.state().execution().window().entries().is_empty());
    assert_eq!(
        departed.state().execution().window().next_entry_sequence(),
        next
    );
}
#[test]
fn accepted_order_prefix_payload_and_lki_remain_exact_when_source_owner_leaves_at_next_target() {
    let state = prepared(BodyTarget::SinglePlayer, 2);
    let first = metadata(&state);
    let selected = choose(&state, TargetRef::Player(C));
    let state = selected.state();
    let second = metadata(state);
    assert_eq!(first.position, 0);
    assert_eq!(second.position, 1);
    assert_eq!(first.wave, second.wave);
    assert_ne!(first.instance, second.instance);
    let capture = state.execution().waiting_triggers().next().unwrap().clone();
    let prefix = state.execution().window().entries()[0].clone();
    let next = state.execution().window().next_entry_sequence();
    let payload = state
        .execution()
        .retained_stack_payloads()
        .find(|payload| payload.entry == first.entry)
        .unwrap();
    let body = payload.body.unwrap().clone();
    let bindings = payload.bindings.unwrap().clone();
    let state = apply(state, B, GameCommand::Concede).into_state();
    assert_eq!(metadata(&state), second);
    assert_eq!(
        state
            .execution()
            .waiting_triggers()
            .cloned()
            .collect::<Vec<_>>(),
        vec![capture]
    );
    assert_eq!(state.execution().window().entries()[0], prefix);
    assert_eq!(state.execution().window().next_entry_sequence(), next);
    let payload = state
        .execution()
        .retained_stack_payloads()
        .find(|payload| payload.entry == first.entry)
        .unwrap();
    assert_eq!(payload.body, Some(&body));
    assert_eq!(payload.bindings, Some(&bindings));
    let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert!(!frame.domain().contains(&TargetRef::Player(B)));
    let selected = choose(&state, TargetRef::Player(D));
    assert!(selected.rules_journal().records.iter().any(
        |record| matches!(record,RulesRecord::TriggerPlaced(placement) if placement==&second)
    ));
    let state = settle(selected.into_state());
    assert_eq!(state.execution().life_total(C), Some(&22.into()));
    assert_eq!(state.execution().life_total(D), Some(&23.into()));
}
#[test]
fn current_target_chooser_departure_cancels_waiting_and_placed_prefix_without_no_target_fiction() {
    let state = prepared(BodyTarget::SinglePlayer, 2);
    let first = metadata(&state);
    let state = choose(&state, TargetRef::Player(C)).into_state();
    let second = metadata(&state);
    let departed = apply(&state, A, GameCommand::Concede);
    assert!(removed(&departed).is_empty());
    assert!(
        departed
            .rules_journal()
            .records
            .iter()
            .all(|record| !matches!(
                record,
                RulesRecord::TriggerTargetSelected { .. } | RulesRecord::TriggerPlaced(_)
            ))
    );
    let ceased = departed
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::PlayersDeparted { ceased_entries, .. } => Some(ceased_entries),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        ceased
            .iter()
            .map(|entry| entry.id())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first.entry, second.entry])
    );
    assert!(departed.state().pending_decision().is_none());
    assert!(departed.state().execution().window().entries().is_empty());
    assert!(
        departed
            .state()
            .execution()
            .waiting_triggers()
            .next()
            .is_none()
    );
    assert_eq!(departed.state().execution().life_total(C), Some(&20.into()));
}

fn cast_free(mut state: GameState, actor: SeatId, id: u64, target: Option<TargetRef>) -> GameState {
    while holder(&state) != actor {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    apply(
        &state,
        actor,
        if let Some(target) = target {
            GameCommand::Cast {
                spell: object(&state, id),
                target,
            }
        } else {
            GameCommand::CastUntargeted {
                spell: object(&state, id),
                payment: None,
            }
        },
    )
    .into_state()
}
fn entry(state: &GameState, card: u64) -> crate::stack_window::StackEntryId {
    state.execution().window().entries().iter().find(|entry|matches!(entry.kind(),crate::stack_window::StackKind::CardSpell(object) if *object==self::object(state,card))).unwrap().id()
}
#[test]
fn actual_spell_loss_refreshes_any_spell_target_or_removes_copyable_target_before_permanent_resolves()
 {
    for filter in [
        SpellTargetFilter::AnySpell,
        SpellTargetFilter::CopyableInstructionSpell,
    ] {
        let mut state = root(
            vec![
                card(
                    1,
                    B,
                    ObjectDestination::Battlefield { controller: A },
                    mana_watcher(BodyTarget::SingleSpell(filter), 1),
                ),
                card(
                    20,
                    C,
                    ObjectDestination::Hand,
                    Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::AdjustControllerLife {
                                adjustment: LifeAdjustment::Gain(2.into()),
                            }],
                        ),
                    ),
                ),
                card(
                    30,
                    A,
                    ObjectDestination::Hand,
                    Program::artifact(Some(ManaCost::free()), vec![]),
                ),
            ],
            &[20; 4],
            Some(Step::PrecombatMain),
            vec![A, B, C, D],
        );
        state = cast_free(state, A, 30, None);
        let permanent = entry(&state, 30);
        state = cast_free(state, C, 20, None);
        let instruction = entry(&state, 20);
        while holder(&state) != A {
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
        state = activate(state);
        let before = metadata(&state);
        let next = state.execution().window().next_entry_sequence();
        let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
            panic!()
        };
        let expected = if filter == SpellTargetFilter::AnySpell {
            vec![TargetRef::Stack(permanent), TargetRef::Stack(instruction)]
        } else {
            vec![TargetRef::Stack(instruction)]
        };
        assert_eq!(frame.domain(), expected);
        state = apply(&state, B, GameCommand::Concede).into_state();
        assert_eq!(metadata(&state), before);
        let departed = apply(&state, C, GameCommand::Concede);
        assert_eq!(
            departed.state().execution().window().next_entry_sequence(),
            next
        );
        assert!(
            departed
                .state()
                .execution()
                .window()
                .entries()
                .iter()
                .all(|entry| entry.id() != instruction)
        );
        let state = if filter == SpellTargetFilter::AnySpell {
            assert!(removed(&departed).is_empty());
            assert_eq!(metadata(departed.state()), before);
            let RuleFrame::TriggerTarget(frame) =
                departed.state().pending_decision().unwrap().frame()
            else {
                panic!()
            };
            assert_eq!(frame.domain(), &[TargetRef::Stack(permanent)]);
            choose(departed.state(), TargetRef::Stack(permanent)).into_state()
        } else {
            assert_eq!(
                removed(&departed),
                vec![(before, TriggerRemovalReason::NoLegalTargets)]
            );
            assert!(departed.state().pending_decision().is_none());
            assert_eq!(
                departed
                    .state()
                    .execution()
                    .window()
                    .entries()
                    .iter()
                    .map(|entry| entry.id())
                    .collect::<Vec<_>>(),
                vec![permanent]
            );
            departed.into_state()
        };
        let state = settle(state);
        assert_eq!(
            state
                .execution()
                .window()
                .cards()
                .get(object(&state, 30))
                .unwrap()
                .zone(),
            if filter == SpellTargetFilter::AnySpell {
                crate::object_identity::ObjectZone::Graveyard(A)
            } else {
                crate::object_identity::ObjectZone::Battlefield
            }
        );
        assert_eq!(state.execution().life_total(C), Some(&20.into()));
    }
}
#[test]
fn actual_noncard_copy_cessation_refreshes_both_spell_target_domains_without_reallocating_trigger()
{
    for filter in [
        SpellTargetFilter::AnySpell,
        SpellTargetFilter::CopyableInstructionSpell,
    ] {
        let mut state = root(
            vec![
                card(
                    1,
                    B,
                    ObjectDestination::Battlefield { controller: A },
                    mana_watcher(BodyTarget::SingleSpell(filter), 1),
                ),
                card(
                    20,
                    C,
                    ObjectDestination::Hand,
                    Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::SingleSpell(SpellTargetFilter::CopyableInstructionSpell),
                            vec![Instruction::CopySpell],
                        ),
                    ),
                ),
                card(
                    30,
                    D,
                    ObjectDestination::Hand,
                    Program::instant(
                        ManaCost::free(),
                        ExecutableBody::new(
                            BodyTarget::None,
                            vec![Instruction::AdjustControllerLife {
                                adjustment: LifeAdjustment::Gain(2.into()),
                            }],
                        ),
                    ),
                ),
            ],
            &[20; 4],
            Some(Step::PrecombatMain),
            vec![A, B, C, D],
        );
        state = cast_free(state, D, 30, None);
        let original = entry(&state, 30);
        state = cast_free(state, C, 20, Some(TargetRef::Stack(original)));
        for _ in 0..20 {
            if !state.execution().window().spell_copies().is_empty() {
                break;
            }
            if let Some(pending) = state.pending_decision() {
                assert!(matches!(pending.frame(), RuleFrame::CopyTargets(_)));
                state = apply(
                    &state,
                    pending.chooser(),
                    GameCommand::Reply {
                        decision: pending.id(),
                        choice: DecisionReply::CopyTargets(CopyTargetChoice::Retain),
                    },
                )
                .into_state();
            } else {
                state = apply(&state, holder(&state), GameCommand::Pass).into_state();
            }
        }
        let copy = state
            .execution()
            .window()
            .entries()
            .iter()
            .find(|entry| {
                matches!(
                    entry.kind(),
                    crate::stack_window::StackKind::SpellCopy { .. }
                )
            })
            .unwrap()
            .id();
        while holder(&state) != A {
            state = apply(&state, holder(&state), GameCommand::Pass).into_state();
        }
        state = activate(state);
        let before = metadata(&state);
        let next = state.execution().window().next_entry_sequence();
        let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
            panic!()
        };
        assert_eq!(
            frame.domain(),
            &[TargetRef::Stack(original), TargetRef::Stack(copy)]
        );
        state = apply(&state, B, GameCommand::Concede).into_state();
        let departed = apply(&state, C, GameCommand::Concede);
        assert!(removed(&departed).is_empty());
        assert_eq!(metadata(departed.state()), before);
        assert_eq!(
            departed.state().execution().window().next_entry_sequence(),
            next
        );
        assert!(
            departed
                .state()
                .execution()
                .window()
                .spell_copies()
                .is_empty()
        );
        let RuleFrame::TriggerTarget(frame) = departed.state().pending_decision().unwrap().frame()
        else {
            panic!()
        };
        assert_eq!(frame.domain(), &[TargetRef::Stack(original)]);
        let chosen = choose(departed.state(), TargetRef::Stack(original));
        assert!(chosen.rules_journal().records.iter().any(
            |record| matches!(record,RulesRecord::TriggerPlaced(placement) if placement==&before)
        ));
        let state = settle(chosen.into_state());
        assert!(state.execution().window().entries().is_empty());
        assert_eq!(state.execution().life_total(C), Some(&20.into()));
        assert_eq!(state.execution().life_total(D), Some(&20.into()));
    }
}

#[test]
fn target_controller_departure_keeps_unrelated_lower_spell_to_resolve_once() {
    let mut state = root(
        vec![
            card(
                1,
                B,
                ObjectDestination::Battlefield { controller: A },
                mana_watcher(BodyTarget::SinglePlayer, 2),
            ),
            card(
                70,
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
            ),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    state = cast_free(state, D, 70, None);
    let lower = entry(&state, 70);
    while holder(&state) != A {
        state = apply(&state, holder(&state), GameCommand::Pass).into_state();
    }
    state = activate(state);
    let first = metadata(&state);
    state = choose(&state, TargetRef::Player(C)).into_state();
    let second = metadata(&state);
    let departed = apply(&state, A, GameCommand::Concede);
    assert!(removed(&departed).is_empty());
    assert!(departed.state().pending_decision().is_none());
    assert_eq!(
        departed
            .state()
            .execution()
            .window()
            .entries()
            .iter()
            .map(|entry| entry.id())
            .collect::<Vec<_>>(),
        vec![lower]
    );
    let ceased = departed
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::PlayersDeparted { ceased_entries, .. } => Some(ceased_entries),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        ceased
            .iter()
            .map(|entry| entry.id())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first.entry, second.entry])
    );
    let state = settle(departed.into_state());
    assert_eq!(state.execution().life_total(C), Some(&20.into()));
    assert_eq!(state.execution().life_total(D), Some(&27.into()));
}

fn prepared_cast_wave() -> GameState {
    let mut cards = vec![card(
        70,
        C,
        ObjectDestination::Hand,
        Program::instant(
            ManaCost::free(),
            ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(2.into()),
                }],
            ),
        ),
    )];
    for (id, owner, controller, target, instruction) in [
        (
            1,
            B,
            A,
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
            Instruction::CounterSpell,
        ),
        (
            2,
            C,
            C,
            BodyTarget::SingleSpell(SpellTargetFilter::AnySpell),
            Instruction::CounterSpell,
        ),
        (
            3,
            D,
            D,
            BodyTarget::None,
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Gain(7.into()),
            },
        ),
    ] {
        let mut source = Program::artifact(None, vec![]);
        source.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::SpellCast {
                player: PlayerRelation::AnyParticipant,
                filter: crate::program::SpellCastFilter::AnySpell,
            },
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(target, vec![instruction]),
        });
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller },
            source,
        ));
    }
    cast_free(
        root(cards, &[20; 4], Some(Step::PrecombatMain), vec![A, B, C, D]),
        C,
        70,
        None,
    )
}

#[test]
fn departing_future_chooser_and_last_stack_target_skip_only_lost_cohort_then_place_survivor() {
    let state = prepared_cast_wave();
    let before = metadata(&state);
    let captured = state
        .execution()
        .waiting_triggers()
        .map(|waiting| (waiting.id, waiting.clone()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(captured.len(), 3);
    let mut exhausted = state.clone();
    exhausted.set_test_check_effect_fault(
        crate::effects::private_qualification::QualificationFault::StackEntry(u64::MAX),
    );
    let saved = exhausted.clone();
    let issued = input(&exhausted, C, GameCommand::Concede);
    let result = exhausted.advance(issued.clone());
    assert_eq!(
        result,
        Err(GameError::Effect(EffectError::Window(
            crate::stack_window::StackWindowError::EntryIdentityExhausted
        )))
    );
    assert_eq!(result, exhausted.advance(issued));
    assert_eq!(exhausted, saved);
    let departed = apply(&state, C, GameCommand::Concede);
    assert_eq!(
        removed(&departed),
        vec![(before.clone(), TriggerRemovalReason::NoLegalTargets)]
    );
    let placed = departed
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerPlaced(placement) => Some(placement),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].controller, D);
    assert_eq!(placed[0].wave, before.wave);
    let expected = captured
        .values()
        .find(|waiting| waiting.controller == D)
        .unwrap();
    assert_eq!(placed[0].instance, expected.id);
    let payload = departed
        .state()
        .execution()
        .retained_stack_payloads()
        .find(|payload| payload.entry == placed[0].entry)
        .unwrap();
    let (body, binding) = expected.clone().into_payload_parts();
    assert_eq!(payload.body, Some(&body));
    assert_eq!(
        payload.bindings,
        Some(&crate::occurrence::ExecutionBindings {
            trigger: Some(binding)
        })
    );
    assert!(departed.state().pending_decision().is_none());
    assert_eq!(holder(departed.state()), D);
    let state = settle(departed.into_state());
    assert_eq!(state.execution().life_total(C), Some(&20.into()));
    assert_eq!(state.execution().life_total(D), Some(&27.into()));
}
#[test]
fn discoveries_during_target_wait_enter_a_later_wave_after_original_choice_finishes() {
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
    let state = activate(root(
        vec![
            card(
                1,
                B,
                ObjectDestination::Battlefield { controller: A },
                mana_watcher(BodyTarget::SinglePlayer, 1),
            ),
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
    ));
    let before = metadata(&state);
    let capture = state.execution().waiting_triggers().next().unwrap().clone();
    let mut exhausted = state.clone();
    exhausted.set_test_check_effect_fault(
        crate::effects::private_qualification::QualificationFault::Instance(u64::MAX),
    );
    let saved = exhausted.clone();
    let issued = input(&exhausted, B, GameCommand::Concede);
    let result = exhausted.advance(issued.clone());
    assert_eq!(
        result,
        Err(GameError::Effect(EffectError::Trigger(
            crate::occurrence::TriggerError::InstanceIdentityExhausted
        )))
    );
    assert_eq!(result, exhausted.advance(issued));
    assert_eq!(exhausted, saved);
    let departed = apply(&state, B, GameCommand::Concede);
    assert_eq!(metadata(departed.state()), before);
    assert!(removed(&departed).is_empty());
    let discovered = departed
        .rules_journal()
        .records
        .iter()
        .find_map(|record| match record {
            RulesRecord::TriggerDiscovered(waiting) => Some(waiting.as_ref().clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(discovered.controller, D);
    assert_eq!(
        departed
            .state()
            .execution()
            .waiting_triggers()
            .find(|waiting| waiting.id == before.instance),
        Some(&capture)
    );
    let mut exhausted = departed.state().clone();
    exhausted.set_test_check_effect_fault(
        crate::effects::private_qualification::QualificationFault::Wave(u64::MAX),
    );
    let saved = exhausted.clone();
    let issued = input(
        &exhausted,
        A,
        GameCommand::Reply {
            decision: exhausted.pending_decision().unwrap().id(),
            choice: DecisionReply::TriggerTarget(TargetRef::Player(C)),
        },
    );
    let result = exhausted.advance(issued.clone());
    assert_eq!(
        result,
        Err(GameError::Effect(EffectError::Trigger(
            crate::occurrence::TriggerError::WaveIdentityExhausted
        )))
    );
    assert_eq!(result, exhausted.advance(issued));
    assert_eq!(exhausted, saved);
    let chosen = choose(departed.state(), TargetRef::Player(C));
    let placements = chosen
        .rules_journal()
        .records
        .iter()
        .filter_map(|record| match record {
            RulesRecord::TriggerPlaced(placement) => Some(placement.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(placements.len(), 2);
    assert_eq!(placements[0], before);
    assert_eq!(placements[1].instance, discovered.id);
    assert_ne!(placements[1].wave, placements[0].wave);
    let payload = chosen
        .state()
        .execution()
        .retained_stack_payloads()
        .find(|payload| payload.entry == placements[1].entry)
        .unwrap();
    let (body, binding) = discovered.into_payload_parts();
    assert_eq!(payload.body, Some(&body));
    assert_eq!(
        payload.bindings,
        Some(&crate::occurrence::ExecutionBindings {
            trigger: Some(binding)
        })
    );
    let state = settle(chosen.into_state());
    assert_eq!(state.execution().life_total(C), Some(&22.into()));
    assert_eq!(state.execution().life_total(D), Some(&27.into()));
}

#[test]
fn structural_target_owner_corruption_rejects_otherwise_valid_reply_without_partial_binding() {
    use crate::effects::private_qualification::QualificationCheckTargetFault as Fault;
    let state = prepared(BodyTarget::SinglePlayer, 2);
    let state = choose(&state, TargetRef::Player(C)).into_state();
    for fault in [
        Fault::DuplicateOrder,
        Fault::ExtendedOrder,
        Fault::PositionPastEnd,
        Fault::WrongEntry,
        Fault::MissingCapture,
        Fault::TargetlessCapture,
        Fault::DifferentSource(object(&state, 20)),
    ] {
        let mut bad = state.clone();
        bad.execution.qualification_corrupt_check_target(fault);
        let saved = bad.clone();
        let issued = input(
            &bad,
            A,
            GameCommand::Reply {
                decision: bad.pending_decision().unwrap().id(),
                choice: DecisionReply::TriggerTarget(TargetRef::Player(D)),
            },
        );
        let result = bad.advance(issued.clone());
        assert_eq!(
            result,
            Err(GameError::Effect(EffectError::InconsistentProgram)),
            "{fault:?}"
        );
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
}
#[test]
fn target_departure_requires_full_issued_authority_and_late_physical_publish_failures_are_atomic() {
    use crate::effects::private_qualification::QualificationFault;
    let state = prepared(BodyTarget::SinglePlayer, 2);
    let state = choose(&state, TargetRef::Player(C)).into_state();
    assert_eq!(
        state
            .execution()
            .depart_players(&BTreeSet::from([B]), state.turn.as_ref(), None),
        Err(EffectError::UnsupportedDepartureState)
    );
    let foreign = prepared(BodyTarget::SingleCreature, 1)
        .pending_decision()
        .unwrap()
        .frame()
        .clone();
    for field in 0..3 {
        let mut bad = state.clone();
        match field {
            0 => bad.pending = None,
            1 => bad.pending.as_mut().unwrap().frame = foreign.clone(),
            2 => bad.pending.as_mut().unwrap().basis.revision += 1,
            _ => unreachable!(),
        };
        let saved = bad.clone();
        let issued = input(&bad, B, GameCommand::Concede);
        let result = bad.advance(issued.clone());
        assert_eq!(result, Err(GameError::UnsupportedConcessionScope));
        assert_eq!(result, bad.advance(issued));
        assert_eq!(bad, saved);
    }
    for fault in [
        QualificationFault::Revision(u64::MAX),
        QualificationFault::Event(u64::MAX),
        QualificationFault::ObjectIdentity(u64::MAX),
        QualificationFault::ObjectRevision(u64::MAX),
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
fn permanent_and_mixed_target_policies_use_fresh_world_and_keep_real_damage_source_controller() {
    for policy in [
        BodyTarget::SinglePermanent,
        BodyTarget::SingleCreatureOrPlayer,
    ] {
        let state = prepared(policy, 1);
        let capture = state.execution().waiting_triggers().next().unwrap().clone();
        let state = apply(&state, B, GameCommand::Concede).into_state();
        let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
            panic!()
        };
        if policy == BodyTarget::SinglePermanent {
            assert_eq!(frame.domain(), &[TargetRef::Object(object(&state, 30))]);
            let state = settle(choose(&state, TargetRef::Object(object(&state, 30))).into_state());
            assert_eq!(
                state
                    .execution()
                    .window()
                    .cards()
                    .get(object(&state, 30))
                    .unwrap()
                    .zone(),
                crate::object_identity::ObjectZone::Hand(D)
            );
        } else {
            assert_eq!(
                frame.domain().iter().copied().collect::<BTreeSet<_>>(),
                BTreeSet::from([
                    TargetRef::Object(object(&state, 30)),
                    TargetRef::Player(A),
                    TargetRef::Player(C),
                    TargetRef::Player(D)
                ])
            );
            let mut state = choose(&state, TargetRef::Player(D)).into_state();
            let mut damage = vec![];
            while !state.execution().window().entries().is_empty() {
                let candidate = apply(&state, holder(&state), GameCommand::Pass);
                damage.extend(candidate.damage_journal().applied.iter().cloned());
                state = candidate.into_state();
            }
            assert_eq!(damage.len(), 1);
            assert_eq!(damage[0].target, crate::damage::DamageRecipient::Player(D));
            assert_eq!(damage[0].amount, 2.into());
            assert_eq!(damage[0].source.controller(), A);
            assert_eq!(
                damage[0].source,
                crate::damage::DamageSource::Object {
                    object: capture.source.object,
                    controller: A
                }
            );
            assert_eq!(state.execution().life_total(D), Some(&18.into()));
        }
    }
}

#[test]
fn ending_departed_static_control_changes_live_source_controller_without_reassigning_trigger_choice()
 {
    use crate::characteristics::{
        ControlRecipient, PermanentSelector, ResolvedDuration, ResolvedPartSpec, ResolvedPartsSpec,
        StaticAbilityId, StaticContinuousAbility, StaticPart, StaticParts,
    };
    let mut source = mana_watcher(BodyTarget::SinglePlayer, 1);
    source.card_types.insert(crate::program::CardType::Artifact);
    source.triggered[0]
        .body
        .instructions
        .push(Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(3.into()),
        });
    let mut static_control = creature();
    static_control.continuous = Some(StaticContinuousAbility::Battlefield {
        id: StaticAbilityId(0),
        selector: PermanentSelector {
            required_types: BTreeSet::from([crate::program::CardType::Artifact]),
            exclude_source: true,
            ..PermanentSelector::default()
        },
        parts: StaticParts(vec![StaticPart::ControlSourceController]),
    });
    let gain_control = || {
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
        )
    };
    let mut state = root(
        vec![
            card(
                1,
                D,
                ObjectDestination::Battlefield { controller: D },
                source,
            ),
            card(
                4,
                B,
                ObjectDestination::Battlefield { controller: B },
                static_control,
            ),
            card(11, C, ObjectDestination::Hand, gain_control()),
            card(12, A, ObjectDestination::Hand, gain_control()),
        ],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    );
    let land = object(&state, 1);
    state = cast_and_resolve(state, C, 11, land);
    let static_source = object(&state, 4);
    state = cast_and_resolve(state, A, 12, static_source);
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(land)
            .unwrap()
            .controller(),
        Some(A)
    );
    state = activate(state);
    let before = metadata(&state);
    let capture = state.execution().waiting_triggers().next().unwrap().clone();
    state = apply(&state, B, GameCommand::Concede).into_state();
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(land)
            .unwrap()
            .controller(),
        Some(C)
    );
    assert_eq!(metadata(&state), before);
    assert_eq!(state.pending_decision().unwrap().chooser(), A);
    assert_eq!(
        state
            .execution()
            .waiting_triggers()
            .cloned()
            .collect::<Vec<_>>(),
        vec![capture]
    );
    let pending = state.pending_decision().unwrap();
    assert_eq!(
        state.advance(input(
            &state,
            C,
            GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::TriggerTarget(TargetRef::Player(D))
            }
        )),
        Err(GameError::WrongChooser)
    );
    let state = settle(choose(&state, TargetRef::Player(D)).into_state());
    assert_eq!(state.execution().life_total(A), Some(&23.into()));
    assert_eq!(state.execution().life_total(C), Some(&20.into()));
    assert_eq!(state.execution().life_total(D), Some(&22.into()));
    assert_eq!(
        state
            .execution()
            .window()
            .cards()
            .get(land)
            .unwrap()
            .controller(),
        Some(C)
    );
}

#[test]
fn departing_current_chooser_advances_to_actual_surviving_target_owner_in_same_wave() {
    let state = prepared_cast_wave();
    let old = metadata(&state);
    let captured = state
        .execution()
        .waiting_triggers()
        .find(|waiting| waiting.controller == C)
        .unwrap()
        .clone();
    let departed = apply(&state, A, GameCommand::Concede);
    assert!(removed(&departed).is_empty());
    let current = metadata(departed.state());
    assert_eq!(current.controller, C);
    assert_eq!(current.instance, captured.id);
    assert_eq!(current.wave, old.wave);
    assert_ne!(current.entry, old.entry);
    assert_eq!(departed.state().pending_decision().unwrap().chooser(), C);
    assert_eq!(
        departed
            .state()
            .execution()
            .waiting_triggers()
            .find(|waiting| waiting.id == captured.id),
        Some(&captured)
    );
    let RuleFrame::TriggerTarget(frame) = departed.state().pending_decision().unwrap().frame()
    else {
        panic!()
    };
    assert_eq!(frame.domain().len(), 1);
    let selected = choose(departed.state(), frame.domain()[0]);
    assert!(selected.rules_journal().records.iter().any(
        |record| matches!(record,RulesRecord::TriggerPlaced(placement) if placement==&current)
    ));
    let state = settle(selected.into_state());
    assert_eq!(state.execution().life_total(C), Some(&20.into()));
    assert_eq!(state.execution().life_total(D), Some(&27.into()));
}
#[test]
fn already_selected_departed_target_fails_at_resolution_without_placement_removal_or_tail() {
    let mut source = mana_watcher(BodyTarget::SinglePlayer, 2);
    source.triggered[0]
        .body
        .instructions
        .push(Instruction::AdjustControllerLife {
            adjustment: LifeAdjustment::Gain(5.into()),
        });
    let state = activate(root(
        vec![card(
            1,
            B,
            ObjectDestination::Battlefield { controller: A },
            source,
        )],
        &[20; 4],
        Some(Step::PrecombatMain),
        vec![A, B, C, D],
    ));
    let first = metadata(&state);
    let state = choose(&state, TargetRef::Player(C)).into_state();
    let prefix = state.execution().window().entries()[0].clone();
    let second = metadata(&state);
    let departed = apply(&state, C, GameCommand::Concede);
    assert!(removed(&departed).is_empty());
    assert_eq!(metadata(departed.state()), second);
    assert_eq!(departed.state().execution().window().entries()[0], prefix);
    let mut state = choose(departed.state(), TargetRef::Player(D)).into_state();
    let mut completions = BTreeMap::new();
    while !state.execution().window().entries().is_empty() {
        let candidate = apply(&state, holder(&state), GameCommand::Pass);
        for record in &candidate.rules_journal().records {
            if let RulesRecord::Event(event) = record {
                for occurrence in &event.occurrences {
                    if let RulesOccurrence::EntryCompleted {
                        entry, completion, ..
                    } = occurrence.value
                    {
                        assert!(completions.insert(entry, completion).is_none());
                    }
                }
            }
        }
        state = candidate.into_state();
    }
    assert_eq!(
        completions,
        BTreeMap::from([
            (
                first.entry,
                crate::occurrence::EntryCompletion::AllTargetsIllegal
            ),
            (second.entry, crate::occurrence::EntryCompletion::Resolved)
        ])
    );
    assert_eq!(state.execution().life_total(A), Some(&20.into()));
    assert_eq!(state.execution().life_total(C), Some(&20.into()));
    assert_eq!(state.execution().life_total(D), Some(&23.into()));
}
