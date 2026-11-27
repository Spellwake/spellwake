//! Prepared public-root checks plus real privileged snapshot/recorder consumers.
//! Synthetic source labels below are unit inputs, never qualification provenance.
use super::*;
use crate::{
    combat::CombatRules,
    effects::SeedCard,
    object_identity::{CardId, ObjectDestination},
    occurrence::RulesRecord,
    program::{Program, TriggerGrouping, TriggeredAbility, TriggeredAbilityId},
    test_support::{
        creature_types,
        current_profile::{
            ObservationJoin, Phase, Recorder, RegistryWitness, SnapshotOwner, SourceRef,
        },
        packet::AliasContext,
        projection,
    },
};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};
const A: SeatId = SeatId(0);
const B: SeatId = SeatId(1);
fn same_source_pending() -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut watcher = Program::artifact(None, vec![]);
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::dies(crate::program::ObjectFilter::creature()),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(
            BodyTarget::SINGLE_PLAYER,
            vec![crate::program::Instruction::AdjustLife(Box::new(
                crate::toolbox::AdjustLifeSpec::gain(
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::TargetPlayer,
                    ),
                    crate::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        ),
    });
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: watcher,
    }];
    // Different graveyard owners avoid a separate graveyard permutation prompt.
    for (id, owner) in [(2, A), (3, B)] {
        cards.push(SeedCard {
            card: CardId(id),
            owner,
            destination: ObjectDestination::Battlefield { controller: owner },
            program: Program::creature(None, 1.into(), 0.into(), BTreeSet::new()),
        });
    }
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1), CardId(2), CardId(3)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let context = GameContext::new(
        "target-pending-unit".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal partial programs".into(),
        "targeted trigger unit".into(),
        "prepared zero-toughness SBA".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    let game = GameState::initialize_scheduled(
        GameId(6041),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::PrecombatMain,
        },
    )
    .unwrap()
    .into_state();
    let pending = game.pending_decision().unwrap();
    let RuleFrame::Triggers(frame) = pending.frame() else {
        panic!("expected same-source order")
    };
    let ids = frame.instances().iter().rev().copied().collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: pending.id(),
            choice: DecisionReply::OrderTriggers(ids),
        },
    })
    .unwrap()
    .into_state()
}
#[test]
fn effect_root_and_actual_inline_witness_preserve_the_cursor_association() {
    let game = same_source_pending();
    let aliases = AliasContext::default();
    let RuleFrame::TriggerTarget(frame) = game.pending_decision().unwrap().frame() else {
        panic!("target frame")
    };
    let waiting = game.execution().waiting_triggers().collect::<Vec<_>>();
    assert_eq!(waiting.len(), 2);
    assert_eq!(waiting[0].source, waiting[1].source);
    assert_eq!(waiting[0].definition, waiting[1].definition);
    assert_ne!(frame.instance(), waiting[0].id);
    assert_eq!(frame.instance(), waiting[1].id);
    let association = game
        .execution()
        .pending_trigger_placement()
        .unwrap()
        .unwrap();
    let effect = projection::effect(game.execution(), &aliases).unwrap();
    let root = projection::state(&game, &aliases).unwrap();
    let encoded = projection::trigger_placement(&association);
    assert_eq!(effect["pending_trigger_placement"], encoded);
    assert_eq!(root["pending_trigger_placement"], encoded);
    assert_eq!(
        root["decision"]["frame"]["frame"]["instance"],
        encoded["instance"]
    );
    assert_eq!(effect["stack"][0]["body"], json!(null));
    assert_eq!(effect["stack"][0]["bindings"], json!(null));
    assert!(
        game.execution().window().entries()[0]
            .targets()
            .targets()
            .is_empty()
    );
    let input = SourceRef {
        path: "synthetic-unit-input".into(),
        sha256: "0".repeat(64),
        bytes: 1,
    };
    let publication = SourceRef {
        path: "synthetic-unit-publication".into(),
        ..input.clone()
    };
    let mut recorder = Recorder::new(
        game.execution().creature_vocabulary(),
        input,
        publication,
        vec![],
        RegistryWitness::NotApplicable,
    )
    .unwrap();
    recorder
        .snapshot(
            "effect",
            &["/observations/effect".into()],
            Phase::Observation,
            "actual pending executor",
            SnapshotOwner::EffectWindow(game.execution()),
            &aliases,
        )
        .unwrap();
    recorder
        .snapshot(
            "root",
            &["/observations/root".into()],
            Phase::Observation,
            "actual pending root",
            SnapshotOwner::GameState(&game),
            &aliases,
        )
        .unwrap();
    recorder
        .finish_coverage(
            vec!["effect".into(), "root".into()],
            vec![],
            vec![
                ObservationJoin {
                    pointer: "/observations/effect".into(),
                    points: vec!["effect".into()],
                },
                ObservationJoin {
                    pointer: "/observations/root".into(),
                    points: vec!["root".into()],
                },
            ],
            vec![],
        )
        .unwrap();
    let witness = recorder
        .witness(&json!({"observations":{"effect":effect,"root":root}}))
        .unwrap();
    assert_eq!(
        witness["schema"],
        "spellwake.current-type-context-witness.v8"
    );
    for row in witness["snapshots"].as_array().unwrap() {
        assert_eq!(
            row["projection"]["value"]["pending_trigger_placement"],
            encoded
        );
    }
    let saved = game.clone();
    let bad = GameInput {
        basis: game.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice: DecisionReply::TriggerTarget(TargetRef::Player(SeatId(99))),
        },
    };
    assert!(game.advance(bad).is_err());
    assert_eq!(game, saved);
    assert_eq!(
        projection::effect(game.execution(), &aliases).unwrap(),
        effect
    );
    assert_eq!(projection::state(&game, &aliases).unwrap(), root);
    let next = game
        .advance(GameInput {
            basis: game.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: game.pending_decision().unwrap().id(),
                choice: DecisionReply::TriggerTarget(TargetRef::Player(B)),
            },
        })
        .unwrap()
        .into_state();
    let next_association = next
        .execution()
        .pending_trigger_placement()
        .unwrap()
        .unwrap();
    assert_ne!(next_association.instance, association.instance);
    assert_ne!(next_association.entry, association.entry);
    assert_eq!(next_association.position, 1);
    let done = next
        .advance(GameInput {
            basis: next.basis().clone(),
            actor: A,
            command: GameCommand::Reply {
                decision: next.pending_decision().unwrap().id(),
                choice: DecisionReply::TriggerTarget(TargetRef::Player(A)),
            },
        })
        .unwrap()
        .into_state();
    assert_eq!(done.execution().pending_trigger_placement().unwrap(), None);
    assert_eq!(
        projection::effect(done.execution(), &aliases).unwrap()["pending_trigger_placement"],
        json!(null)
    );
}

fn cleanup_pending(no_targets: bool) -> GameState {
    let vocabulary = creature_types::selected_vocabulary().unwrap();
    let mut watcher = Program::artifact(None, vec![]);
    let (target, body) = if no_targets {
        (
            BodyTarget::SINGLE_CREATURE,
            vec![crate::program::Instruction::DealDamage(Box::new(
                crate::toolbox::DealDamageSpec {
                    source: crate::toolbox::DamageSourceQuery::EntrySource,
                    recipients: crate::toolbox::DamageTargetQuery::Targets,
                    amount: crate::program::DamageAmountSpec::Expr(
                        crate::program::ValueExpr::Literal(1.into()),
                    ),
                    exile_if_dies: false,
                },
            ))],
        )
    } else {
        (
            BodyTarget::SINGLE_PLAYER,
            vec![crate::program::Instruction::AdjustLife(Box::new(
                crate::toolbox::AdjustLifeSpec::gain(
                    crate::toolbox::PlayerSelector::Resolving(
                        crate::program::ResolvingPlayer::TargetPlayer,
                    ),
                    crate::program::ValueExpr::Literal(1.into()),
                ),
            ))],
        )
    };
    watcher.triggered.push(TriggeredAbility {
        id: TriggeredAbilityId(1),
        condition: crate::program::TriggerSpec::player_action(
            crate::program::PlayerRelation::Controller,
            crate::program::PlayerTriggerAction::Discarded,
        ),
        grouping: TriggerGrouping::EachOccurrence,
        body: ExecutableBody::new(target, body),
    });
    let mut cards = vec![SeedCard {
        card: CardId(1),
        owner: A,
        destination: ObjectDestination::Battlefield { controller: A },
        program: watcher,
    }];
    for id in 2..=9 {
        cards.push(SeedCard {
            card: CardId(id),
            owner: A,
            destination: ObjectDestination::Hand,
            program: Program::artifact(None, vec![]),
        });
    }
    let seed = GameSeed::with_uniform_life(
        vec![A, B],
        A,
        cards,
        vec![CardId(1)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        vocabulary,
        Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let context = GameContext::new(
        "target-cleanup-unit".into(),
        seed.creature_vocabulary.rules_revision().into(),
        "literal partial programs".into(),
        "targeted cleanup unit".into(),
        "prepared cleanup with eight hand cards".into(),
        seed.creature_vocabulary.id(),
        seed.name_rules.id(),
    )
    .unwrap();
    GameState::initialize_scheduled(
        GameId(6042),
        context,
        seed,
        TurnStart {
            number: 3,
            step: Step::Cleanup,
        },
    )
    .unwrap()
    .into_state()
}
fn reply_to(game: &GameState, choice: DecisionReply) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor: game.pending_decision().unwrap().chooser(),
        command: GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice,
        },
    })
    .unwrap()
}
fn pass(game: &GameState, actor: SeatId) -> NativeCandidate {
    game.advance(GameInput {
        basis: game.basis().clone(),
        actor,
        command: GameCommand::Pass,
    })
    .unwrap()
}
#[test]
fn cleanup_hands_off_to_priority_before_target_choice_and_always_runs_extra_cleanup() {
    for no_targets in [false, true] {
        let game = cleanup_pending(no_targets);
        let cleanup = game.turn().unwrap().position();
        assert_eq!(
            game.turn().unwrap().stage(),
            crate::turn::TurnStage::Work(crate::turn::TurnWork::Cleanup)
        );
        let RuleFrame::Checks(crate::checks::CheckDecision::CleanupDiscard(frame)) =
            game.pending_decision().unwrap().frame()
        else {
            panic!("cleanup discard")
        };
        // Inspect genuine Cleanup completion before the root's Priority handoff.
        let (cleanup_done, cleanup_result, cleanup_append) = game
            .execution()
            .resume_rule(
                game.execution().revision(),
                &EffectDecision::Checks(crate::checks::CheckDecision::CleanupDiscard(
                    frame.clone(),
                )),
                A,
                DecisionReply::Discard(vec![frame.domain()[0]]),
                game.turn(),
            )
            .unwrap()
            .into_parts();
        let RuleResult::Complete {
            completion: RuleCompletion::Checks(completion),
            ..
        } = cleanup_result
        else {
            panic!("genuine Cleanup must finish before placement")
        };
        assert_eq!(
            completion.boundary(),
            crate::checks::CheckBoundary::Cleanup(cleanup)
        );
        assert!(!completion.performed());
        assert!(completion.waiting_triggers());
        assert_eq!(cleanup_done.waiting_triggers().count(), 1);
        assert!(cleanup_done.window().entries().is_empty());
        assert!(!cleanup_append.records.iter().any(|record| matches!(
            record,
            RulesRecord::TriggerPlacementStarted(_)
                | RulesRecord::TriggerPlaced(_)
                | RulesRecord::TriggerRemoved { .. }
        )));
        let discarded = reply_to(&game, DecisionReply::Discard(vec![frame.domain()[0]]));
        let starts = discarded
            .rules_journal()
            .records
            .iter()
            .filter_map(|record| {
                if let RulesRecord::TriggerPlacementStarted(placement) = record {
                    Some(placement.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let [started] = starts.as_slice() else {
            panic!("exactly one actual placement attempt")
        };
        assert_eq!(
            discarded
                .rules_journal()
                .records
                .iter()
                .filter(|record| matches!(record, RulesRecord::TriggerRemoved { .. }))
                .count(),
            usize::from(no_targets)
        );
        assert!(
            !discarded
                .rules_journal()
                .records
                .iter()
                .any(|record| matches!(
                    record,
                    RulesRecord::TriggerPlaced(_) | RulesRecord::TriggerTargetSelected { .. }
                ))
        );
        if no_targets {
            assert!(discarded.rules_journal().records.iter().any(|record| matches!(record, RulesRecord::TriggerRemoved { placement, reason: crate::occurrence::TriggerRemovalReason::NoLegalTargets } if placement == started)));
        }
        let started = started.clone();
        let mut state = discarded.into_state();
        assert_eq!(state.turn().unwrap().position(), cleanup);
        assert_eq!(
            state.turn().unwrap().stage(),
            crate::turn::TurnStage::Priority
        );
        if no_targets {
            assert!(state.pending_decision().is_none());
            assert!(state.execution().window().entries().is_empty());
        } else {
            let RuleFrame::TriggerTarget(frame) = state.pending_decision().unwrap().frame() else {
                panic!("cleanup exception target")
            };
            assert_eq!(frame.boundary(), crate::checks::CheckBoundary::Priority);
            assert_eq!(
                frame.domain(),
                &[TargetRef::Player(A), TargetRef::Player(B)]
            );
            // The direct Priority executor interface does not gain a calendar requirement.
            let original = state.clone();
            assert!(
                state
                    .execution()
                    .resume_rule(
                        state.execution().revision(),
                        &EffectDecision::TriggerTarget(frame.clone()),
                        A,
                        DecisionReply::TriggerTarget(TargetRef::Player(B)),
                        None
                    )
                    .is_ok()
            );
            assert_eq!(state, original);
            let selected = reply_to(&state, DecisionReply::TriggerTarget(TargetRef::Player(B)));
            assert_eq!(selected.rules_journal().records.iter().filter(|record| matches!(record, RulesRecord::TriggerPlaced(placement) if placement == &started)).count(), 1);
            assert_eq!(selected.rules_journal().records.iter().filter(|record| matches!(record, RulesRecord::TriggerTargetSelected { placement, .. } if placement == &started)).count(), 1);
            assert!(
                !selected
                    .rules_journal()
                    .records
                    .iter()
                    .any(|record| matches!(
                        record,
                        RulesRecord::TriggerPlacementStarted(_)
                            | RulesRecord::TriggerRemoved { .. }
                    ))
            );
            state = selected.into_state();
            state = pass(&state, A).into_state();
            state = pass(&state, B).into_state();
            assert_eq!(state.execution().life_total(B), Some(&21.into()));
            assert_eq!(state.turn().unwrap().position(), cleanup);
        }
        assert_eq!(state.execution().waiting_triggers().count(), 0);
        assert_eq!(state.execution().pending_trigger_placement().unwrap(), None);
        state = pass(&state, A).into_state();
        let advanced = pass(&state, B);
        assert!(
            advanced
                .turn_transitions()
                .iter()
                .any(|row| row.to.step == Step::Cleanup && row.to.cleanup_iteration == 1)
        );
    }
}

#[test]
fn direct_target_frames_validate_every_captured_field_without_changing_the_owner() {
    let game = same_source_pending();
    let execution = game.execution();
    let original = execution.clone();
    let RuleFrame::TriggerTarget(frame) = game.pending_decision().unwrap().frame() else {
        panic!()
    };
    let other = execution
        .waiting_triggers()
        .find(|waiting| waiting.id != frame.instance())
        .unwrap()
        .id;
    let mut wrongs = vec![frame.qualification_with_next_wave().unwrap()];
    let mut wrong = frame.clone();
    wrong.revision += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.event.pass += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.event.started_revision += 1;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.chooser = B;
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.instance = other;
    wrongs.push(wrong);
    let later = reply_to(&game, DecisionReply::TriggerTarget(TargetRef::Player(B))).into_state();
    let RuleFrame::TriggerTarget(later_frame) = later.pending_decision().unwrap().frame() else {
        panic!("next actual target")
    };
    let mut wrong = frame.clone();
    wrong.entry = later_frame.entry();
    wrongs.push(wrong);
    let mut wrong = frame.clone();
    wrong.domain.reverse();
    wrongs.push(wrong);
    for wrong in wrongs {
        assert_eq!(
            execution.resume_rule(
                execution.revision(),
                &EffectDecision::TriggerTarget(wrong),
                A,
                DecisionReply::TriggerTarget(TargetRef::Player(B)),
                None
            ),
            Err(EffectError::Trigger(
                crate::occurrence::TriggerError::WrongTargetFrame
            ))
        );
        assert_eq!(execution, &original);
    }
    assert_eq!(
        execution.resume_rule(
            execution.revision(),
            &EffectDecision::TriggerTarget(frame.clone()),
            B,
            DecisionReply::TriggerTarget(TargetRef::Player(B)),
            None
        ),
        Err(EffectError::WrongChooser)
    );
    assert_eq!(execution, &original);
    let mut wrong = frame.clone();
    wrong.boundary = crate::checks::CheckBoundary::Cleanup(game.turn().unwrap().position());
    assert_eq!(
        execution.resume_rule(
            execution.revision(),
            &EffectDecision::TriggerTarget(wrong),
            A,
            DecisionReply::TriggerTarget(TargetRef::Player(B)),
            None
        ),
        Err(EffectError::WrongPhase)
    );
    assert_eq!(execution, &original);
}

#[test]
fn late_root_decision_exhaustion_keeps_the_accepted_pending_prefix_intact() {
    let mut game = same_source_pending();
    game.next_decision = u64::MAX;
    let saved = game.clone();
    let aliases = AliasContext::default();
    let before = projection::state(&game, &aliases).unwrap();
    let input = GameInput {
        basis: game.basis().clone(),
        actor: A,
        command: GameCommand::Reply {
            decision: game.pending_decision().unwrap().id(),
            choice: DecisionReply::TriggerTarget(TargetRef::Player(B)),
        },
    };
    assert_eq!(
        game.advance(input),
        Err(GameError::DecisionIdentityExhausted)
    );
    assert_eq!(game, saved);
    assert_eq!(projection::state(&game, &aliases).unwrap(), before);
    assert_eq!(game.execution().waiting_triggers().count(), 2);
}
