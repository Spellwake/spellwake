#![allow(dead_code, unused_imports)]
//! Conditional effects use real paid casts, root decisions and one live cursor.
use super::*;
use spellwake_engine::program::{Comparison, CreatureTypeSlot, ResolutionCondition, ValueExpr};
fn branch(left: ValueExpr, comparison: Comparison, right: i64, destination: usize) -> Instruction {
    Instruction::BranchCondition {
        condition: Box::new(spellwake_engine::toolbox::ConditionExpr::Resolution(
            Box::new(ResolutionCondition {
                left,
                comparison,
                right: spellwake_engine::program::ValueExpr::Literal(right.into()),
            }),
        )),
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
                Instruction::PromptChoice(Box::new(
                    spellwake_engine::toolbox::PromptChoiceSpec::choose_creature_type(
                        slot,
                        BTreeSet::new(),
                    ),
                )),
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
                choice: DecisionReply::PromptChoice(
                    spellwake_engine::toolbox::PromptChoiceReply::CreatureType(human),
                ),
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
    let done = tr::reply(
        paused.state(),
        P0,
        DecisionReply::PromptChoice(spellwake_engine::toolbox::PromptChoiceReply::CreatureType(
            human,
        )),
    );
    let saved_done = done.state().clone();
    assert!(
        done.state()
            .advance(GameInput {
                basis: done.state().basis().clone(),
                actor: P0,
                command: GameCommand::Reply {
                    decision,
                    choice: DecisionReply::PromptChoice(
                        spellwake_engine::toolbox::PromptChoiceReply::CreatureType(human),
                    ),
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
                attacking_defender: None,
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
