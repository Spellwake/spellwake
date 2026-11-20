//! Real target-frame departure, source capture and opaque Stack handle reconstruction.
use super::checks_order::{app, departure, order, project};
use super::chooser_replacement::{bound, finish_original_stack, four_seat_root, mechanism_step};
use super::*;
use spellwake_engine::{
    effects::CopyTargetChoice,
    game::{NativeCandidate, RuleFrame},
    program::{
        ObjectRelation, OrdinaryCondition, SpellTargetFilter, TriggerGrouping, TriggeredAbility,
        TriggeredAbilityId,
    },
};
const C: SeatId = SeatId(29);
const D: SeatId = SeatId(77);
fn watcher(target: BodyTarget, count: u64) -> Program {
    let mut source = Program::land(
        BTreeSet::new(),
        vec![ManaAbility {
            id: ManaAbilityId(7),
            output: ManaOutput::Fixed(BTreeMap::from([(ManaType::Red, 1.into())])),
        }],
    );
    for id in 1..=count {
        source.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(id),
            condition: OrdinaryCondition::BecomesTapped(ObjectRelation::SelfObject),
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(
                target,
                vec![match target {
                    BodyTarget::SingleCreature => Instruction::ReturnToOwnersHand,
                    BodyTarget::SinglePlayer => Instruction::AdjustTargetLife {
                        adjustment: LifeAdjustment::Gain(i64::try_from(id + 1).unwrap().into()),
                    },
                    BodyTarget::SingleSpell(_) => Instruction::CounterSpell,
                    _ => panic!("finite target profile"),
                }],
            ),
        });
    }
    source
}
fn activate(mut source: GameState) -> GameState {
    source = mechanism_step(
        &source,
        A,
        GameCommand::ActivateMana {
            source: object(&source, 1),
            ability: ManaAbilityRef::Printed(ManaAbilityId(7)),
        },
    );
    if matches!(
        source.pending_decision().unwrap().frame(),
        RuleFrame::Triggers(_)
    ) {
        source = order(&source);
    }
    assert!(matches!(
        source.pending_decision().unwrap().frame(),
        RuleFrame::TriggerTarget(_)
    ));
    source
}
fn prepare(target: BodyTarget, count: u64) -> GameState {
    activate(four_seat_root(vec![
        card(
            1,
            B,
            ObjectDestination::Battlefield { controller: A },
            watcher(target, count),
        ),
        card(
            20,
            B,
            ObjectDestination::Battlefield { controller: B },
            Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
        ),
        card(
            30,
            D,
            ObjectDestination::Battlefield { controller: D },
            Program::creature(None, 2.into(), 3.into(), BTreeSet::new()),
        ),
    ]))
}
fn native_target(scope: &NativeScope<'_>, target: &PlayerTarget) -> TargetRef {
    match target {
        PlayerTarget::Seat { seat_id } => TargetRef::Player(
            scope
                .binding
                .seats
                .iter()
                .find(|seat| seat.seat_id == *seat_id)
                .unwrap()
                .native_seat,
        ),
        PlayerTarget::Object { handle } => {
            let NativeReference::Object(id) = scope.reference(handle).unwrap() else {
                panic!("object")
            };
            TargetRef::Object(id)
        }
        PlayerTarget::Stack { handle } => {
            let NativeReference::Stack(id) = scope.reference(handle).unwrap() else {
                panic!("stack")
            };
            TargetRef::Stack(id)
        }
    }
}
fn original(
    scope: &NativeScope<'_>,
    descriptor: &PlayerDecision,
    wanted: TargetRef,
) -> PlayerCommand {
    assert!(descriptor.kind == DecisionKind::TriggerTarget);
    let PlayerDomain::Targets { targets } = &descriptor.domain else {
        panic!("target domain")
    };
    let target = targets
        .iter()
        .find(|target| native_target(scope, target) == wanted)
        .unwrap()
        .clone();
    command(
        scope,
        PlayerAction::Choose {
            decision: descriptor.decision.clone(),
            response: PlayerChoice::Target { target },
        },
    )
}
fn choose(source: &GameState, wanted: TargetRef) -> NativeCandidate {
    let actor = source.pending_decision().unwrap().chooser();
    let viewer = app(source, actor);
    let mut scope = NativeScope::new(source, bound(source, viewer)).unwrap();
    let snapshot = project(&mut scope);
    let descriptor = &snapshot.snapshot().decisions[0];
    assert!(
        matches!(crate::native_game::clocks::waiting(source,&scope.binding).unwrap(),crate::native_game::clocks::Waiting::Decision(seat) if seat==viewer)
    );
    let command = original(&scope, descriptor, wanted);
    let roster = snapshot
        .snapshot()
        .players
        .iter()
        .map(|seat| seat.seat_id)
        .collect::<Vec<_>>();
    descriptor
        .validate_response(&command, viewer, &roster, &[])
        .unwrap();
    let prepared = scope.propose(&command).unwrap();
    input_codec::verify(&scope, &prepared);
    let bytes = scope.encode_input(&prepared).unwrap();
    assert!(
        restore_input(
            prepared.candidate().state(),
            bound(prepared.candidate().state(), viewer),
            &bytes
        )
        .is_err()
    );
    if matches!(wanted, TargetRef::Stack(_)) {
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            source.execution().window().entries().len() >= 2,
            "index one is actual in-range provisional trigger"
        );
        for location in [
            serde_json::json!({"kind":"stack","index":"1"}),
            serde_json::json!({"kind":"object","index":"0"}),
        ] {
            let mut wrong = record.clone();
            let row = wrong["transition"]["references"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|row| row["location"]["kind"] == "stack")
                .unwrap();
            assert_eq!(row["location"]["index"], "0");
            row["location"] = location;
            assert!(
                restore_input(
                    source,
                    scope.binding.clone(),
                    &serde_json::to_vec(&wrong).unwrap()
                )
                .is_err()
            );
        }
    }
    let observer = if actor == D { A } else { D };
    let mut other = NativeScope::new(source, bound(source, app(source, observer))).unwrap();
    assert!(project(&mut other).snapshot().decisions.is_empty());
    assert!(other.propose(&command).is_err());
    prepared.candidate().clone()
}
fn observe_finished(source: &GameState, viewer: SeatId) {
    assert!(source.pending_decision().is_none());
    let mut scope = NativeScope::new(source, bound(source, app(source, viewer))).unwrap();
    assert!(project(&mut scope).snapshot().decisions.is_empty());
}
fn no_fake_target(candidate: &NativeCandidate) {
    assert!(
        candidate
            .rules_journal()
            .records
            .iter()
            .all(|record| !matches!(
                record,
                spellwake_engine::occurrence::RulesRecord::TriggerTargetSelected { .. }
                    | spellwake_engine::occurrence::RulesRecord::TriggerPlaced(_)
            ))
    );
}
#[test]
#[ignore = "requires genuine vocabulary; object target refresh and no-target native originals"]
fn object_target_departure_originals_refresh_single_choice_or_remove_provisional_without_fake_input()
 {
    for omit in [false, true] {
        let source = prepare(BodyTarget::SingleCreature, 1);
        let before = source
            .execution()
            .pending_trigger_placement()
            .unwrap()
            .unwrap();
        let departed = departure(&source, B);
        no_fake_target(&departed);
        let source = departed.into_state();
        assert_eq!(
            source.execution().pending_trigger_placement().unwrap(),
            Some(before.clone())
        );
        if omit {
            let departed = departure(&source, D);
            no_fake_target(&departed);
            let actual = departed
                .rules_journal()
                .records
                .iter()
                .filter_map(|record| match record {
                    spellwake_engine::occurrence::RulesRecord::TriggerRemoved {
                        placement,
                        reason,
                    } => Some((placement.clone(), *reason)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                actual,
                vec![(
                    before,
                    spellwake_engine::occurrence::TriggerRemovalReason::NoLegalTargets
                )]
            );
            assert!(departed.state().pending_decision().is_none());
            assert!(departed.state().execution().window().entries().is_empty());
            observe_finished(departed.state(), C);
        } else {
            let chosen = choose(&source, TargetRef::Object(object(&source, 30)));
            let source = finish_original_stack(chosen.into_state());
            assert_eq!(
                source
                    .execution()
                    .window()
                    .cards()
                    .get(object(&source, 30))
                    .unwrap()
                    .zone(),
                ObjectZone::Hand(D)
            );
            observe_finished(&source, A);
        }
    }
}
#[test]
#[ignore = "requires genuine vocabulary; player targets and source/chooser departure originals"]
fn player_target_prefix_originals_preserve_lki_or_cancel_gone_chooser_without_fake_target() {
    for cancel in [false, true] {
        let source = prepare(BodyTarget::SinglePlayer, 2);
        let first = source
            .execution()
            .pending_trigger_placement()
            .unwrap()
            .unwrap();
        let source = choose(&source, TargetRef::Player(C)).into_state();
        let second = source
            .execution()
            .pending_trigger_placement()
            .unwrap()
            .unwrap();
        assert_eq!(second.position, 1);
        let departed = departure(&source, if cancel { A } else { B });
        no_fake_target(&departed);
        if cancel {
            assert!(departed.state().pending_decision().is_none());
            let entries = departed
                .rules_journal()
                .records
                .iter()
                .find_map(|record| match record {
                    spellwake_engine::occurrence::RulesRecord::PlayersDeparted {
                        ceased_entries,
                        ..
                    } => Some(ceased_entries),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                entries
                    .iter()
                    .map(|entry| entry.id())
                    .collect::<BTreeSet<_>>(),
                BTreeSet::from([first.entry, second.entry])
            );
            assert_eq!(departed.state().execution().life_total(C), Some(&20.into()));
            observe_finished(departed.state(), D);
        } else {
            assert_eq!(
                departed
                    .state()
                    .execution()
                    .pending_trigger_placement()
                    .unwrap(),
                Some(second)
            );
            let chosen = choose(departed.state(), TargetRef::Player(D));
            let source = finish_original_stack(chosen.into_state());
            assert_eq!(source.execution().life_total(C), Some(&23.into()));
            assert_eq!(source.execution().life_total(D), Some(&22.into()));
            observe_finished(&source, A);
        }
    }
}
fn cast(mut source: GameState, actor: SeatId, id: u64, target: Option<TargetRef>) -> GameState {
    loop {
        let spellwake_engine::stack_window::WindowPhase::Priority(
            spellwake_engine::priority::PriorityPhase::Ready { holder },
        ) = source.execution().window().phase()
        else {
            panic!("priority")
        };
        if holder == actor {
            break;
        }
        source = mechanism_step(&source, holder, GameCommand::Pass);
    }
    mechanism_step(
        &source,
        actor,
        if let Some(target) = target {
            GameCommand::Cast {
                spell: object(&source, id),
                target,
            }
        } else {
            GameCommand::CastUntargeted {
                spell: object(&source, id),
                payment: None,
            }
        },
    )
}
fn copy_source(filter: SpellTargetFilter) -> GameState {
    let mut source = four_seat_root(vec![
        card(
            1,
            B,
            ObjectDestination::Battlefield { controller: A },
            watcher(BodyTarget::SingleSpell(filter), 1),
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
    ]);
    source = cast(source, D, 30, None);
    let original = source.execution().window().entries()[0].id();
    source = cast(source, C, 20, Some(TargetRef::Stack(original)));
    for _ in 0..20 {
        if !source.execution().window().spell_copies().is_empty() {
            break;
        }
        if let Some(pending) = source.pending_decision() {
            assert!(matches!(pending.frame(), RuleFrame::CopyTargets(_)));
            source = mechanism_step(
                &source,
                pending.chooser(),
                GameCommand::Reply {
                    decision: pending.id(),
                    choice: spellwake_engine::effects::DecisionReply::CopyTargets(
                        CopyTargetChoice::Retain,
                    ),
                },
            );
        } else {
            let spellwake_engine::stack_window::WindowPhase::Priority(
                spellwake_engine::priority::PriorityPhase::Ready { holder },
            ) = source.execution().window().phase()
            else {
                panic!()
            };
            source = mechanism_step(&source, holder, GameCommand::Pass);
        }
    }
    assert_eq!(source.execution().window().spell_copies().len(), 1);
    loop {
        let spellwake_engine::stack_window::WindowPhase::Priority(
            spellwake_engine::priority::PriorityPhase::Ready { holder },
        ) = source.execution().window().phase()
        else {
            panic!()
        };
        if holder == A {
            break;
        }
        source = mechanism_step(&source, holder, GameCommand::Pass);
    }
    activate(source)
}
#[test]
#[ignore = "requires genuine vocabulary; real copy cessation and opaque Stack target originals"]
fn stack_target_originals_drop_real_copy_refresh_handles_and_refuse_in_range_provisional_alias() {
    for filter in [
        SpellTargetFilter::AnySpell,
        SpellTargetFilter::CopyableInstructionSpell,
    ] {
        let source = copy_source(filter);
        let before = source
            .execution()
            .pending_trigger_placement()
            .unwrap()
            .unwrap();
        let copy = source
            .execution()
            .window()
            .entries()
            .iter()
            .find(|entry| {
                matches!(
                    entry.kind(),
                    spellwake_engine::stack_window::StackKind::SpellCopy { .. }
                )
            })
            .unwrap()
            .id();
        let mut old = NativeScope::new(&source, bound(&source, viewer_a())).unwrap();
        let snapshot = project(&mut old);
        let probe = old
            .propose(&original(
                &old,
                &snapshot.snapshot().decisions[0],
                TargetRef::Stack(copy),
            ))
            .unwrap();
        let stale = old.encode_input(&probe).unwrap();
        let source = departure(&source, B).into_state();
        let source = departure(&source, C).into_state();
        assert!(source.execution().window().spell_copies().is_empty());
        assert_eq!(
            source.execution().pending_trigger_placement().unwrap(),
            Some(before)
        );
        assert!(restore_input(&source, bound(&source, viewer_a()), &stale).is_err());
        let RuleFrame::TriggerTarget(frame) = source.pending_decision().unwrap().frame() else {
            panic!()
        };
        assert_eq!(frame.domain().len(), 1);
        let chosen = choose(&source, frame.domain()[0]);
        let source = finish_original_stack(chosen.into_state());
        assert!(source.execution().window().entries().is_empty());
        assert_eq!(source.execution().life_total(C), Some(&20.into()));
        assert_eq!(source.execution().life_total(D), Some(&20.into()));
        observe_finished(&source, A);
    }
}

#[test]
#[ignore = "requires genuine vocabulary; surviving chooser uses original roster and Stack handles"]
fn departing_chooser_migrates_target_wait_to_surviving_controller_with_exact_replay() {
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
        let mut program = Program::artifact(None, vec![]);
        program.triggered.push(TriggeredAbility {
            id: TriggeredAbilityId(1),
            condition: OrdinaryCondition::SpellCast {
                player: spellwake_engine::program::PlayerRelation::AnyParticipant,
                filter: spellwake_engine::program::SpellCastFilter::AnySpell,
            },
            grouping: TriggerGrouping::EachOccurrence,
            body: ExecutableBody::new(target, vec![instruction]),
        });
        cards.push(card(
            id,
            owner,
            ObjectDestination::Battlefield { controller },
            program,
        ));
    }
    let source = cast(four_seat_root(cards), C, 70, None);
    assert_eq!(source.pending_decision().unwrap().chooser(), A);
    let before = source
        .execution()
        .pending_trigger_placement()
        .unwrap()
        .unwrap();
    let expected = source
        .execution()
        .waiting_triggers()
        .find(|capture| capture.controller == C)
        .unwrap()
        .clone();
    let departed = departure(&source, A);
    assert!(
        !departed
            .rules_journal()
            .records
            .iter()
            .any(|record| matches!(
                record,
                spellwake_engine::occurrence::RulesRecord::TriggerRemoved { .. }
            ))
    );
    let source = departed.into_state();
    let next = source
        .execution()
        .pending_trigger_placement()
        .unwrap()
        .unwrap();
    assert_eq!(next.wave, before.wave);
    assert_eq!(next.controller, C);
    assert_ne!(next.entry, before.entry);
    assert_eq!(
        source
            .execution()
            .waiting_triggers()
            .find(|capture| capture.id == expected.id),
        Some(&expected)
    );
    let RuleFrame::TriggerTarget(frame) = source.pending_decision().unwrap().frame() else {
        panic!()
    };
    assert_eq!(frame.domain().len(), 1);
    let chosen = choose(&source, frame.domain()[0]);
    let source = finish_original_stack(chosen.into_state());
    assert!(source.execution().window().entries().is_empty());
    assert_eq!(source.execution().life_total(C), Some(&20.into()));
    assert_eq!(source.execution().life_total(D), Some(&27.into()));
    observe_finished(&source, C);
}
