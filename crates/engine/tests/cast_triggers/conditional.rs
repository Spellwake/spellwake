//! Conditional effects use real paid casts, root decisions and one live cursor.
use super::*;
use spellwake_engine::program::{Comparison, CreatureTypeSlot, ResolutionCondition, ValueExpr};
fn branch(left: ValueExpr, comparison: Comparison, right: i64, destination: usize) -> Instruction {
    Instruction::BranchIfFalse {
        condition: ResolutionCondition {
            left,
            comparison,
            right: spellwake_engine::program::ValueExpr::Literal(right.into()),
        },
        destination,
    }
}
fn life(threshold: i64, destination: usize) -> Instruction {
    branch(
        spellwake_engine::program::ValueExpr::PlayerStat(
            spellwake_engine::program::PlayerStatExpr::ControllerLife,
        ),
        Comparison::GreaterOrEqual,
        threshold,
        destination,
    )
}
fn spell(target: BodyTarget, ops: Vec<Instruction>) -> Program {
    Program::instant(
        ManaCost::one(ManaType::Blue),
        ExecutableBody::new(target, ops),
    )
}
fn hand(id: u64, owner: SeatId, program: Program) -> SeedCard {
    SeedCard {
        card: CardId(id),
        owner,
        destination: ObjectDestination::Hand,
        program,
    }
}
fn branches(candidate: &NativeCandidate) -> Vec<(usize, String, String, bool, usize)> {
    candidate
        .rules_journal()
        .records
        .iter()
        .filter_map(|row| match row {
            RulesRecord::InstructionBranch {
                instruction,
                left,
                right,
                result,
                next,
                ..
            } => Some((
                *instruction,
                String::from(left.clone()),
                String::from(right.clone()),
                *result,
                *next,
            )),
            _ => None,
        })
        .collect()
}
#[test]
fn conditional_true_false_and_changed_current_life_execute_only_selected_paths() {
    for (ops, total, expected) in [
        (
            vec![life(20, 2), gain(2), gain(1)],
            23,
            vec![(0, "20".into(), "20".into(), true, 1)],
        ),
        (
            vec![life(21, 2), gain(2), gain(1)],
            21,
            vec![(0, "20".into(), "21".into(), false, 2)],
        ),
        (
            vec![gain(5), life(25, 3), gain(2), gain(1)],
            28,
            vec![(1, "25".into(), "25".into(), true, 2)],
        ),
        (
            vec![
                Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Set((-5).into()),
                },
                branch(
                    spellwake_engine::program::ValueExpr::PlayerStat(
                        spellwake_engine::program::PlayerStatExpr::ControllerLife,
                    ),
                    Comparison::Equal,
                    -5,
                    3,
                ),
                gain(30),
            ],
            25,
            vec![(1, "-5".into(), "-5".into(), true, 2)],
        ),
    ] {
        let game = two(vec![hand(1, P0, spell(BodyTarget::None, ops))]);
        let cast = paid(&game, P0, 1, 0, None);
        let done = resolve(cast.state());
        assert_eq!(done.state().execution().life_total(P0), Some(&total.into()));
        assert_eq!(branches(&done), expected);
        assert!(done.state().execution().window().entries().is_empty());
        assert!(
            done.state()
                .execution()
                .window()
                .participants()
                .contains(&P0)
        );
    }
}
#[test]
fn jump_skips_else_and_body_end_completes_without_a_leaf_or_magic_event() {
    for (threshold, total, next) in [(20, 22, 1), (21, 27, 3)] {
        let game = two(vec![hand(
            1,
            P0,
            spell(
                BodyTarget::None,
                vec![
                    life(threshold, 3),
                    gain(2),
                    Instruction::Jump { destination: 4 },
                    gain(7),
                ],
            ),
        )]);
        let cast = paid(&game, P0, 1, 0, None);
        let done = resolve(cast.state());
        assert_eq!(done.state().execution().life_total(P0), Some(&total.into()));
        assert_eq!(branches(&done)[0].4, next);
        assert_eq!(
            done.rules_journal()
                .records
                .iter()
                .filter(|row| matches!(
                    row,
                    RulesRecord::InstructionJump {
                        instruction: 2,
                        next: 4,
                        ..
                    }
                ))
                .count(),
            usize::from(threshold == 20)
        );
    }
    let game = two(vec![hand(
        1,
        P0,
        spell(BodyTarget::None, vec![Instruction::Jump { destination: 1 }]),
    )]);
    let cast = paid(&game, P0, 1, 0, None);
    let done = resolve(cast.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(
        tr::all_occurrences(&done)
            .iter()
            .filter(|value| matches!(value, RulesOccurrence::LifeChanged(_)))
            .count(),
        0
    );
    assert_eq!(
        done.rules_journal()
            .records
            .iter()
            .filter(|row| matches!(
                row,
                RulesRecord::InstructionJump {
                    instruction: 0,
                    next: 1,
                    ..
                }
            ))
            .count(),
        1
    );
}
#[test]
fn mutually_exclusive_type_producers_keep_one_cursor_and_join_the_actual_local() {
    let slot = CreatureTypeSlot::new(0);
    let v = tr::vocabulary();
    let human = v.resolve_label("Human").unwrap();
    for threshold in [20, 21] {
        let choice = || Instruction::ChooseCreatureType {
            into: slot,
            excluding: BTreeSet::new(),
        };
        let program = spell(
            BodyTarget::None,
            vec![
                life(threshold, 3),
                choice(),
                Instruction::Jump { destination: 4 },
                choice(),
                Instruction::DrawController {
                    amount: spellwake_engine::program::ValueExpr::count_battlefield(
                        spellwake_engine::program::ObjectFilter::any()
                            .controlled()
                            .of_chosen_slot(slot),
                    ),
                },
                gain(1),
            ],
        );
        let game = two(vec![
            hand(1, P0, program),
            tr::controlled(
                2,
                P0,
                P0,
                Program::creature(None, 1.into(), 1.into(), BTreeSet::from([human])),
            ),
        ]);
        let cast = paid(&game, P0, 1, 0, None);
        let paused = resolve(cast.state());
        let index = if threshold == 20 { 1 } else { 3 };
        assert_eq!(paused.state().execution().instruction_index(), Some(index));
        assert_eq!(branches(&paused).len(), 1);
        let pending = paused.state().pending_decision().unwrap();
        let RuleFrame::CreatureType(frame) = pending.frame() else {
            panic!("type choice")
        };
        assert_eq!(frame.instruction(), index);
        let input = GameInput {
            basis: paused.state().basis().clone(),
            actor: P0,
            command: GameCommand::Reply {
                decision: pending.id(),
                choice: DecisionReply::CreatureType(human),
            },
        };
        let saved = paused.state().clone();
        let done = paused.state().advance(input.clone()).unwrap();
        assert_eq!(paused.state().advance(input).unwrap(), done);
        assert_eq!(paused.state(), &saved);
        assert_eq!(done.state().execution().life_total(P0), Some(&21.into()));
        assert_eq!(done.draw_journal().events.len(), 1);
        assert!(done.state().execution().window().entries().is_empty());
        assert_eq!(
            done.rules_journal()
                .records
                .iter()
                .filter(|row| matches!(row, RulesRecord::InstructionJump { .. }))
                .count(),
            usize::from(threshold == 20)
        );
    }
}
#[test]
fn wrong_and_stale_type_replies_preserve_branch_paused_state() {
    let slot = CreatureTypeSlot::new(0);
    let human = tr::vocabulary().resolve_label("Human").unwrap();
    let game = two(vec![hand(
        1,
        P0,
        spell(
            BodyTarget::None,
            vec![
                life(20, 2),
                Instruction::ChooseCreatureType {
                    into: slot,
                    excluding: BTreeSet::new(),
                },
                gain(1),
            ],
        ),
    )]);
    let cast = paid(&game, P0, 1, 0, None);
    let paused = resolve(cast.state());
    let saved = paused.state().clone();
    let decision = paused.state().pending_decision().unwrap().id();
    for (actor, command) in [
        (
            P1,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::CreatureType(human),
            },
        ),
        (P0, GameCommand::Pass),
        (
            P0,
            GameCommand::Reply {
                decision,
                choice: DecisionReply::OrderTriggers(vec![]),
            },
        ),
    ] {
        assert!(
            paused
                .state()
                .advance(GameInput {
                    basis: paused.state().basis().clone(),
                    actor,
                    command
                })
                .is_err()
        );
        assert_eq!(paused.state(), &saved);
    }
    let done = tr::reply(paused.state(), P0, DecisionReply::CreatureType(human));
    let saved_done = done.state().clone();
    assert!(
        done.state()
            .advance(GameInput {
                basis: done.state().basis().clone(),
                actor: P0,
                command: GameCommand::Reply {
                    decision,
                    choice: DecisionReply::CreatureType(human)
                }
            })
            .is_err()
    );
    assert_eq!(done.state(), &saved_done);
}
#[test]
fn matched_count_condition_uses_captured_causes_after_source_departure() {
    let mut watcher = Program::creature(None, 1.into(), 0.into(), BTreeSet::new());
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::dies(
            spellwake_engine::program::ObjectFilter::creature(),
        ),
        grouping: TriggerGrouping::OneOrMoreTogether,
        body: ExecutableBody::new(
            BodyTarget::None,
            vec![
                branch(
                    spellwake_engine::program::ValueExpr::event(
                        spellwake_engine::program::EventValueKind::MatchedOccurrences,
                    ),
                    Comparison::Equal,
                    2,
                    2,
                ),
                gain(4),
            ],
        ),
    });
    // One simultaneous state-based event kills both creatures; only the watcher has an ability.
    let game = two(vec![
        tr::controlled(1, P0, P0, watcher),
        tr::controlled(
            2,
            P1,
            P1,
            Program::creature(None, 1.into(), 0.into(), BTreeSet::new()),
        ),
    ]);
    let done = resolve(&game);
    assert_eq!(branches(&done), vec![(0, "2".into(), "2".into(), true, 1)]);
    assert_eq!(done.state().execution().life_total(P0), Some(&24.into()));
}

#[test]
fn all_illegal_targets_stop_before_any_branch_or_untargeted_tail() {
    let game = two(vec![
        hand(
            1,
            P0,
            spell(
                BodyTarget::SingleCreature,
                vec![life(21, 2), Instruction::ReturnToOwnersHand, gain(1)],
            ),
        ),
        hand(
            3,
            P1,
            spell(
                BodyTarget::SingleCreature,
                vec![Instruction::ReturnToOwnersHand],
            ),
        ),
        tr::controlled(
            2,
            P1,
            P1,
            Program::creature(None, 2.into(), 2.into(), BTreeSet::new()),
        ),
    ]);
    let target = tr::object(&game, 2);
    let cast = paid(&game, P0, 1, 0, Some(TargetRef::Object(target)));
    let passed = tr::submit(cast.state(), P0, GameCommand::Pass);
    let response = paid(passed.state(), P1, 3, 0, Some(TargetRef::Object(target)));
    let bounced = resolve(response.state());
    let done = resolve(bounced.state());
    assert!(branches(&done).is_empty());
    assert!(tr::all_occurrences(&done).iter().any(|row| matches!(
        row,
        RulesOccurrence::ZoneChange(change)
            if change.reason == spellwake_engine::occurrence::ZoneReason::AllTargetsIllegalExit
    )));
    assert_eq!(done.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(
        done.state()
            .execution()
            .window()
            .cards()
            .get(tr::object(done.state(), 2))
            .unwrap()
            .zone(),
        ObjectZone::Hand(P1)
    );
}
#[test]
fn token_timestamp_pause_restores_control_cursor_then_jumps_over_else() {
    use spellwake_engine::token::{TokenCreation, TokenDefinition, TokenName, TokenRecipients};
    let v = tr::vocabulary();
    let definition = TokenDefinition::new(
        Program::creature(None, 1.into(), 1.into(), BTreeSet::new()),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let ops = vec![
        life(20, 3),
        Instruction::CreateTokens {
            creations: vec![TokenCreation {
                definition,
                amount: spellwake_engine::program::ValueExpr::Literal(2.into()),
                base_size: None,
                enters_tapped: false,
                recipients: TokenRecipients::Controller,
            }],
        },
        Instruction::Jump { destination: 4 },
        gain(99),
        gain(1),
    ];
    let game = two(vec![hand(1, P0, spell(BodyTarget::None, ops))]);
    let cast = paid(&game, P0, 1, 0, None);
    let paused = resolve(cast.state());
    let RuleFrame::TimestampOrder(frame) = paused.state().pending_decision().unwrap().frame()
    else {
        panic!("token timestamp choice")
    };
    assert_eq!(paused.state().execution().instruction_index(), Some(1));
    assert_eq!(frame.members().len(), 2);
    assert_eq!(branches(&paused).len(), 1);
    let mut order = frame
        .members()
        .iter()
        .map(|row| row.member)
        .collect::<Vec<_>>();
    order.reverse();
    let done = tr::reply(paused.state(), P0, DecisionReply::TimestampOrder(order));
    assert_eq!(done.state().execution().life_total(P0), Some(&21.into()));
    assert_eq!(
        tr::all_occurrences(&done)
            .iter()
            .filter(|row| matches!(row, RulesOccurrence::TokenCreated { .. }))
            .count(),
        2
    );
    assert!(done.rules_journal().records.iter().any(|row| matches!(
        row,
        RulesRecord::InstructionJump {
            instruction: 2,
            next: 4,
            ..
        }
    )));
}
#[test]
fn damage_pause_restores_control_cursor_then_jumps_over_else() {
    use spellwake_engine::effects::DamageDecision;
    let shield = || {
        spell(
            BodyTarget::SingleCreature,
            vec![Instruction::CreatePreventionShield { amount: 1.into() }],
        )
    };
    let damage = spell(
        BodyTarget::SingleCreature,
        vec![
            life(20, 3),
            Instruction::Damage { amount: 4.into() },
            Instruction::Jump { destination: 4 },
            gain(99),
            gain(1),
        ],
    );
    let game = two(vec![
        hand(1, P0, shield()),
        hand(2, P0, shield()),
        hand(3, P0, damage),
        tr::controlled(
            4,
            P1,
            P1,
            Program::creature(None, 10.into(), 10.into(), BTreeSet::new()),
        ),
    ]);
    let target = TargetRef::Object(tr::object(&game, 4));
    let one = paid(&game, P0, 1, 0, Some(target));
    let one = resolve(one.state());
    let two = paid(one.state(), P0, 2, 1, Some(target));
    let two = resolve(two.state());
    let cast = paid(two.state(), P0, 3, 2, Some(target));
    let paused = resolve(cast.state());
    let RuleFrame::Damage(DamageDecision::Order(frame)) =
        paused.state().pending_decision().unwrap().frame()
    else {
        panic!("damage order")
    };
    assert_eq!(paused.state().execution().instruction_index(), Some(1));
    assert_eq!(branches(&paused).len(), 1);
    assert_eq!(frame.applications().len(), 2);
    let done = tr::reply(
        paused.state(),
        P1,
        DecisionReply::DamageOrder(frame.applications()[0]),
    );
    assert_eq!(done.state().execution().life_total(P0), Some(&21.into()));
    assert_eq!(
        done.state()
            .execution()
            .battlefield_marked_damage(tr::object(done.state(), 4)),
        Some(2.into())
    );
    assert!(done.rules_journal().records.iter().any(|row| matches!(
        row,
        RulesRecord::InstructionJump {
            instruction: 2,
            next: 4,
            ..
        }
    )));
}
#[test]
fn copy_target_pause_restores_control_cursor_then_jumps_over_else() {
    use spellwake_engine::effects::CopyTargetChoice;
    let original = spell(
        BodyTarget::SinglePlayer,
        vec![Instruction::AdjustTargetLife {
            adjustment: LifeAdjustment::Gain(2.into()),
        }],
    );
    let copy = spell(
        BodyTarget::SingleSpell(spellwake_engine::program::SpellFilter::copyable_instruction()),
        vec![
            life(20, 3),
            Instruction::CopySpell,
            Instruction::Jump { destination: 4 },
            gain(99),
            gain(1),
        ],
    );
    let game = two(vec![hand(1, P0, original), hand(2, P0, copy)]);
    let cast = paid(&game, P0, 1, 0, Some(TargetRef::Player(P1)));
    let original = cast
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let copied = paid(cast.state(), P0, 2, 1, Some(TargetRef::Stack(original)));
    let paused = resolve(copied.state());
    let RuleFrame::CopyTargets(frame) = paused.state().pending_decision().unwrap().frame() else {
        panic!("copy targets")
    };
    assert_eq!(frame.instruction_index(), 1);
    assert_eq!(branches(&paused).len(), 1);
    let done = tr::reply(
        paused.state(),
        P0,
        DecisionReply::CopyTargets(CopyTargetChoice::Change(TargetRef::Player(P0))),
    );
    assert_eq!(done.state().execution().life_total(P0), Some(&21.into()));
    assert!(done.rules_journal().records.iter().any(|row| matches!(
        row,
        RulesRecord::InstructionJump {
            instruction: 2,
            next: 4,
            ..
        }
    )));
    let copy = resolve(done.state());
    let original = resolve(copy.state());
    assert_eq!(
        original.state().execution().life_total(P0),
        Some(&23.into())
    );
    assert_eq!(
        original.state().execution().life_total(P1),
        Some(&22.into())
    );
}

#[test]
fn an_executed_draw_from_an_empty_library_is_attempted_and_a_skipped_draw_is_absent() {
    use spellwake_engine::draw::{DrawEvent, GameOutcome};
    for threshold in [20, 21] {
        let draw = |n: i64| Instruction::DrawController {
            amount: spellwake_engine::program::ValueExpr::Literal(n.into()),
        };
        let game = two(vec![hand(
            1,
            P0,
            spell(
                BodyTarget::None,
                vec![draw(8), life(threshold, 3), draw(1), gain(1)],
            ),
        )]);
        let cast = paid(&game, P0, 1, 0, None);
        let done = resolve(cast.state());
        assert_eq!(done.state().execution().life_total(P0), Some(&21.into()));
        assert_eq!(
            done.draw_journal()
                .events
                .iter()
                .filter(|row| matches!(row, DrawEvent::Drawn { .. }))
                .count(),
            8
        );
        assert_eq!(done.draw_journal().events.iter().filter(|row|matches!(row,DrawEvent::EmptyLibrary{player,attempts}if *player==P0 && *attempts==1.into())).count(),usize::from(threshold==20));
        assert_eq!(
            done.state().outcome(),
            if threshold == 20 {
                Some(GameOutcome::Winner(P1))
            } else {
                None
            }
        );
        assert_eq!(
            branches(&done)[0],
            (
                1,
                "20".into(),
                threshold.to_string(),
                threshold == 20,
                if threshold == 20 { 2 } else { 3 }
            )
        );
    }
}

#[test]
fn controller_life_uses_captured_trigger_controller_after_source_control_changes() {
    use spellwake_engine::characteristics::{ControlRecipient, ResolvedDuration, ResolvedPartSpec};
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: spellwake_engine::program::TriggerSpec::spell_cast(
            PlayerRelation::Controller,
            spellwake_engine::program::SpellFilter::any(),
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(BodyTarget::None, vec![life(20, 2), gain(4)]),
    });
    let take = spell(
        BodyTarget::SinglePermanent,
        vec![
            tr::effect(
                vec![ResolvedPartSpec::Control(
                    ControlRecipient::ResolvingController,
                )],
                ResolvedDuration::NoStatedDuration,
            ),
            Instruction::AdjustControllerLife {
                adjustment: LifeAdjustment::Set(7.into()),
            },
        ],
    );
    // The watcher is owned by P1, controlled by P0 at discovery, then controlled
    // by P1 with a different life total before its independent trigger resolves.
    let game = two(vec![
        hand(1, P0, instruction()),
        tr::controlled(2, P1, P0, watcher),
        hand(3, P1, take),
    ]);
    let source = tr::object(&game, 2);
    let cast = paid(&game, P0, 1, 0, None);
    let captured = discoveries(&cast)[0].clone();
    assert_eq!(captured.controller, P0);
    assert_eq!(captured.source.object.owner(), P1);
    let trigger = cast
        .state()
        .execution()
        .window()
        .entries()
        .last()
        .unwrap()
        .id();
    let passed = tr::submit(cast.state(), P0, GameCommand::Pass);
    let response = paid(passed.state(), P1, 3, 0, Some(TargetRef::Object(source)));
    let changed = resolve(response.state());
    assert_eq!(tr::current(changed.state(), source).controller, Some(P1));
    assert_eq!(changed.state().execution().life_total(P0), Some(&20.into()));
    assert_eq!(changed.state().execution().life_total(P1), Some(&7.into()));
    let entry = changed
        .state()
        .execution()
        .window()
        .entries()
        .iter()
        .find(|entry| entry.id() == trigger)
        .unwrap();
    assert_eq!(entry.controller(), P0);
    let binding = changed
        .state()
        .execution()
        .execution_bindings(trigger)
        .unwrap()
        .trigger
        .as_ref()
        .unwrap();
    assert_eq!(binding.source, captured.source);
    assert_eq!(binding.causes, captured.causes);
    let retained = changed.state().clone();
    let ability = resolve(changed.state());
    assert_eq!(resolve(changed.state()), ability);
    assert_eq!(changed.state(), &retained);
    assert_eq!(
        branches(&ability),
        vec![(0, "20".into(), "20".into(), true, 1)]
    );
    assert_eq!(ability.state().execution().life_total(P0), Some(&24.into()));
    assert_eq!(ability.state().execution().life_total(P1), Some(&7.into()));
    let done = resolve(ability.state());
    assert_eq!(done.state().execution().life_total(P0), Some(&26.into()));
    assert_eq!(done.state().execution().life_total(P1), Some(&7.into()));
}
