//! Actual root rollback after progressively later token-creation failures.
use super::*;
use crate::{
    effects::SeedCard,
    land::{BasicLandType, LandType},
    life::LifeAdjustment,
    mana::{ManaAbilityRef, ManaCost, ManaType},
    object_identity::{CardId, ObjectDestination},
    program::{Instruction, PlayerRelation, Program, TriggeredAbility, TriggeredAbilityId},
    token::{TokenDefinition, TokenName},
};
fn prepare() -> GameState {
    prepare_count(1)
}
fn prepare_count(count: i64) -> GameState {
    prepare_grouped_count(count, crate::program::TriggerGrouping::EachOccurrence)
}
fn prepare_grouped_count(count: i64, grouping: crate::program::TriggerGrouping) -> GameState {
    let v = crate::test_support::creature_types::selected_vocabulary().unwrap();
    let definition = TokenDefinition::new(
        Program::artifact(None, vec![]),
        TokenName::Default {
            ordered_subtypes: vec![],
        },
        &v,
    )
    .unwrap();
    let producer = Program::instant(
        ManaCost::specific(ManaType::Green, 1.into()),
        ExecutableBody::new(
            BodyTarget::None,
            vec![Instruction::CreateTokens {
                creations: vec![crate::token::TokenCreation {
                    definition,
                    amount: crate::program::ValueExpr::Literal(count.into()),
                    base_size: None,
                    enters_tapped: false,
                    recipients: crate::token::TokenRecipients::Controller,
                }],
            }],
        ),
    );
    let mut observer = Program::artifact(None, vec![]);
    for id in [7, 8] {
        observer.triggered.push(TriggeredAbility {
            grouping,
            id: TriggeredAbilityId(id),
            condition: crate::program::TriggerSpec::player_action(
                PlayerRelation::Controller,
                crate::program::PlayerTriggerAction::TokenCreated,
            ),
            body: ExecutableBody::new(
                BodyTarget::None,
                vec![Instruction::AdjustControllerLife {
                    adjustment: LifeAdjustment::Gain(1.into()),
                }],
            ),
        });
    }
    let a = SeatId(0);
    let b = SeatId(1);
    let context = GameContext::new(
        "token-faults".into(),
        v.rules_revision().into(),
        "literal".into(),
        "tokens".into(),
        "root-rollback".into(),
        v.id(),
        spellwake_contracts::names::NameRules::literal_names().id(),
    )
    .unwrap();
    let seed = GameSeed::with_uniform_life(
        vec![a, b],
        a,
        vec![
            SeedCard {
                card: CardId(1),
                owner: a,
                destination: ObjectDestination::Hand,
                program: producer,
            },
            SeedCard {
                card: CardId(10),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: Program::land(BTreeSet::from([LandType::Forest]), vec![]),
            },
            SeedCard {
                card: CardId(20),
                owner: a,
                destination: ObjectDestination::Battlefield { controller: a },
                program: observer,
            },
        ],
        vec![CardId(10), CardId(20)],
        vec![],
        20.into(),
        CombatRules::UnrestrictedFreeForAll,
        v,
        std::sync::Arc::new(spellwake_contracts::names::NameRules::literal_names()),
    );
    let mut state = GameState::initialize(GameId(20261102), context, seed)
        .unwrap()
        .into_state();
    let source = state
        .execution
        .window()
        .cards()
        .current_identity(CardId(10))
        .unwrap();
    state = state
        .advance(GameInput {
            basis: state.basis.clone(),
            actor: a,
            command: GameCommand::ActivateMana {
                source,
                ability: ManaAbilityRef::BasicLand(BasicLandType::Forest),
            },
        })
        .unwrap()
        .into_state();
    let spell = state
        .execution
        .window()
        .cards()
        .current_identity(CardId(1))
        .unwrap();
    state = state
        .advance(GameInput {
            basis: state.basis.clone(),
            actor: a,
            command: GameCommand::CastUntargeted {
                spell,
                payment: None,
            },
        })
        .unwrap()
        .into_state();
    state
        .advance(GameInput {
            basis: state.basis.clone(),
            actor: a,
            command: GameCommand::Pass,
        })
        .unwrap()
        .into_state()
}
#[test]
fn token_late_failures_roll_back_insertion_timestamp_publication_and_root_decision() {
    for fault in 0..6 {
        let mut state = prepare();
        match fault {
            0 => state.execution.set_test_object_allocator(u64::MAX, true),
            1 => state.execution.set_test_object_allocator(u64::MAX, false),
            2 => state.execution.set_test_characteristic_timestamp(u64::MAX),
            3 => state.execution.set_test_publication_event(u64::MAX),
            4 => state.execution.set_test_publication_instance(u64::MAX),
            _ => state.next_decision = u64::MAX,
        }
        let before = state.clone();
        let result = state.advance(GameInput {
            basis: state.basis.clone(),
            actor: SeatId(1),
            command: GameCommand::Pass,
        });
        assert!(result.is_err(), "fault {fault} unexpectedly accepted");
        assert_eq!(state, before, "fault {fault} changed an accepted ancestor");
        assert!(
            state
                .execution
                .window()
                .cards()
                .iter()
                .all(|object| !object.is_token())
        );
        assert_eq!(state.execution.window().entries().len(), 1);
    }
}

#[test]
fn token_batch_commit_failures_preserve_the_whole_pending_order_and_root() {
    for fault in 0..6 {
        let before_order = prepare_count(2);
        let mut state = before_order
            .advance(GameInput {
                basis: before_order.basis.clone(),
                actor: SeatId(1),
                command: GameCommand::Pass,
            })
            .unwrap()
            .into_state();
        let decision = state.pending_decision().unwrap().clone();
        let RuleFrame::TimestampOrder(frame) = decision.frame() else {
            panic!("batch ordering")
        };
        let order = frame.members().iter().map(|m| m.member).collect();
        match fault {
            0 => state.execution.set_test_object_allocator(u64::MAX, true),
            1 => state.execution.set_test_object_allocator(u64::MAX, false),
            2 => state.execution.set_test_characteristic_timestamp(u64::MAX),
            3 => state.execution.set_test_publication_event(u64::MAX),
            4 => state.execution.set_test_publication_instance(u64::MAX),
            _ => state.next_decision = u64::MAX,
        }
        let saved = state.clone();
        assert!(
            state
                .advance(GameInput {
                    basis: state.basis.clone(),
                    actor: SeatId(0),
                    command: GameCommand::Reply {
                        decision: decision.id(),
                        choice: DecisionReply::TimestampOrder(order)
                    }
                })
                .is_err(),
            "fault {fault}"
        );
        assert_eq!(state, saved);
        assert!(
            state
                .execution
                .window()
                .cards()
                .iter()
                .all(|o| !o.is_token())
        );
    }
}
#[test]
fn token_batch_exact_native_frame_and_lower_creator_admission_are_not_player_authority() {
    let before_order = prepare_count(2);
    let state = before_order
        .advance(GameInput {
            basis: before_order.basis.clone(),
            actor: SeatId(1),
            command: GameCommand::Pass,
        })
        .unwrap()
        .into_state();
    let RuleFrame::TimestampOrder(frame) = state.pending_decision().unwrap().frame() else {
        panic!()
    };
    let order = frame.members().iter().map(|m| m.member).collect::<Vec<_>>();
    for mutation in 0..4 {
        let mut bad = frame.clone();
        match mutation {
            0 => bad.revision += 1,
            1 => bad.instruction += 1,
            2 => bad.chooser = SeatId(1),
            _ => bad.members[0].creator = SeatId(1),
        }
        let saved = state.execution.clone();
        assert!(
            state
                .execution
                .resume_rule(
                    state.execution.revision(),
                    &crate::effects::EffectDecision::TimestampOrder(bad),
                    SeatId(0),
                    DecisionReply::TimestampOrder(order.clone()),
                    state.turn.as_ref()
                )
                .is_err()
        );
        assert_eq!(state.execution, saved);
    }
    let window = state.execution.window();
    let saved = window.clone();
    let result = window.record_token_creations(
        window.revision(),
        vec![
            (SeatId(0), frame.members()[0].definition.clone()),
            (SeatId(99), frame.members()[1].definition.clone()),
        ],
        state.execution.creature_vocabulary(),
    );
    assert!(result.is_err());
    assert_eq!(window, &saved);
}

#[test]
fn late_grouped_instance_failure_preserves_pending_batch_and_all_ancestors() {
    let prepared = prepare_grouped_count(2, crate::program::TriggerGrouping::OneOrMoreTogether);
    let mut pending = prepared
        .advance(GameInput {
            basis: prepared.basis.clone(),
            actor: SeatId(1),
            command: GameCommand::Pass,
        })
        .unwrap()
        .into_state();
    let decision = pending.pending_decision().unwrap().clone();
    let RuleFrame::TimestampOrder(frame) = decision.frame() else {
        panic!("real batch frame");
    };
    let order = frame.members().iter().map(|m| m.member).collect();
    pending
        .execution
        .set_test_publication_instance(u64::MAX - 1);
    let saved = pending.clone();
    assert_eq!(
        pending.advance(GameInput {
            basis: pending.basis.clone(),
            actor: SeatId(0),
            command: GameCommand::Reply {
                decision: decision.id(),
                choice: DecisionReply::TimestampOrder(order)
            }
        }),
        Err(GameError::Effect(EffectError::Trigger(
            crate::occurrence::TriggerError::InstanceIdentityExhausted
        )))
    );
    assert_eq!(pending, saved);
    assert_eq!(pending.execution.waiting_triggers().count(), 0);
    assert!(
        pending
            .execution
            .window()
            .cards()
            .iter()
            .all(|o| !o.is_token())
    );
}
